/**
 * xterm.js bound to a real pty in Rust.
 *
 * The pty outlives this component: switching to another Depot tab hides the
 * editor rather than unmounting it, and even a remount reattaches to the same
 * session id, so a running command keeps running.
 */
import { useEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { api, onTermData, onTermExit } from "../api";
import { Icon } from "../lib/icons";

const LIGHT = {
  background: "#fdfdfc",
  foreground: "#1b1c1e",
  cursor: "#3b74d1",
  cursorAccent: "#fdfdfc",
  selectionBackground: "#dbe7fa",
  black: "#45453f",
  red: "#cc4436",
  green: "#2f8455",
  yellow: "#a8802a",
  blue: "#3566bd",
  magenta: "#7d5bd0",
  cyan: "#2b8f8f",
  white: "#6b6b67",
  brightBlack: "#92928d",
  brightRed: "#e0574a",
  brightGreen: "#3f9d68",
  brightYellow: "#c19638",
  brightBlue: "#4a7fd1",
  brightMagenta: "#9070e0",
  brightCyan: "#37a5a5",
  brightWhite: "#1b1c1e",
};

const DARK = {
  background: "#1d1e21",
  foreground: "#e9eaec",
  cursor: "#5f92e4",
  cursorAccent: "#1d1e21",
  selectionBackground: "#2c4676",
  black: "#34373b",
  red: "#eb7264",
  green: "#58bd85",
  yellow: "#d4ac54",
  blue: "#6d9fea",
  magenta: "#a68bec",
  cyan: "#4bbcbc",
  white: "#a4a8ae",
  brightBlack: "#7b7f86",
  brightRed: "#f38a7d",
  brightGreen: "#72d09b",
  brightYellow: "#e4c070",
  brightBlue: "#8fb7f0",
  brightMagenta: "#bda4f2",
  brightCyan: "#68d0d0",
  brightWhite: "#ffffff",
};

const decode = (b64: string) => {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
};

/**
 * Every mount gets its own pty. Reattaching to a session that is already
 * running would leave the fresh xterm blank — the scrollback lives in the
 * widget that was just torn down — and it makes React's double-invoked effects
 * in development race the close against the open.
 */
let ptySeq = 0;

export function TerminalPanel({
  id,
  cwd,
  theme,
  visible,
  onExit,
  onError,
}: {
  /** Stable across remounts — it is the pty session key on the Rust side. */
  id: string;
  cwd: string;
  theme: "light" | "dark";
  /** False while the tab is parked; xterm cannot measure a hidden element. */
  visible: boolean;
  onExit?: (code: number | null) => void;
  onError: (message: string) => void;
}) {
  const holder = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  const fit = useRef<FitAddon | null>(null);
  const [dead, setDead] = useState(false);
  const [generation, setGeneration] = useState(0);
  const callbacks = useRef({ onExit, onError });
  callbacks.current = { onExit, onError };
  // Read through a ref, never a dependency: re-rooting the workspace must not
  // tear down a shell that is in the middle of running something. A restart
  // after a re-root does pick up the new folder.
  const startDir = useRef(cwd);
  startDir.current = cwd;

  useEffect(() => {
    const host = holder.current;
    if (!host) return;

    const ptyId = `${id}#${++ptySeq}`;
    const terminal = new Terminal({
      fontFamily:
        'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
      fontSize: 12.5,
      lineHeight: 1.25,
      cursorBlink: true,
      allowProposedApi: true,
      scrollback: 5000,
      theme: theme === "dark" ? DARK : LIGHT,
    });
    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.loadAddon(new WebLinksAddon());
    terminal.open(host);
    term.current = terminal;
    fit.current = fitAddon;

    const keystrokes = terminal.onData((data) => {
      void api.termWrite(ptyId, data).catch(() => setDead(true));
    });
    const resizes = terminal.onResize(({ cols, rows }) => {
      void api.termResize(ptyId, cols, rows).catch(() => {});
    });

    let disposed = false;
    const unlisteners: Array<() => void> = [];
    setDead(false);

    // Size the pty to the pane before the shell prints its first prompt.
    try {
      fitAddon.fit();
    } catch {
      /* zero-sized while the pane is still laying out; the observer retries */
    }

    // Both listeners must be live before the shell starts, or its prompt is
    // emitted into the void — `listen` is itself an async IPC round trip.
    void (async () => {
      try {
        const stop = [
          await onTermData((e) => {
            if (e.id === ptyId && !disposed) terminal.write(decode(e.chunk));
          }),
          await onTermExit((e) => {
            if (e.id !== ptyId || disposed) return;
            setDead(true);
            terminal.write(
              `\r\n\x1b[2m[process exited${e.code == null ? "" : ` · code ${e.code}`}]\x1b[0m\r\n`,
            );
            callbacks.current.onExit?.(e.code);
          }),
        ];
        if (disposed) {
          stop.forEach((un) => un());
          return;
        }
        unlisteners.push(...stop);
        await api.termOpen(ptyId, startDir.current, terminal.cols || 80, terminal.rows || 24);
      } catch (e) {
        if (disposed) return;
        setDead(true);
        callbacks.current.onError(String(e));
      }
    })();

    const observer = new ResizeObserver(() => {
      if (!host.clientWidth || !host.clientHeight) return;
      try {
        fitAddon.fit();
      } catch {
        /* transient zero size */
      }
    });
    observer.observe(host);

    return () => {
      disposed = true;
      observer.disconnect();
      keystrokes.dispose();
      resizes.dispose();
      unlisteners.forEach((un) => un());
      terminal.dispose();
      term.current = null;
      fit.current = null;
      void api.termClose(ptyId).catch(() => {});
    };
    // `generation` restarts the session after the shell exits. `cwd` is
    // deliberately absent — see `startDir`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, generation]);

  useEffect(() => {
    term.current?.options && (term.current.options.theme = theme === "dark" ? DARK : LIGHT);
  }, [theme]);

  // Becoming visible again: the element had no size while hidden, so xterm's
  // cached geometry is stale until it measures once more.
  useEffect(() => {
    if (!visible) return;
    const raf = requestAnimationFrame(() => {
      try {
        fit.current?.fit();
        term.current?.focus();
      } catch {
        /* still laying out */
      }
    });
    return () => cancelAnimationFrame(raf);
  }, [visible]);

  return (
    <div className="term">
      <div className="term-surface" ref={holder} />
      {dead && (
        <button className="term-restart" onClick={() => setGeneration((g) => g + 1)}>
          <Icon name="reload" size={14} />
          Restart shell
        </button>
      )}
    </div>
  );
}
