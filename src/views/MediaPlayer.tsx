import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, fileUrl } from "../api";
import { formatDuration, playsInWebview } from "../lib/files";
import { Icon, type IconName } from "../lib/icons";
import type { SourceKind, SubtitleTrack } from "../types";

const RATES = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];

export function MediaPlayer({
  title,
  path,
  source,
  ext,
  kind,
  parked = false,
  onError,
}: {
  title: string;
  path: string;
  source: SourceKind;
  ext: string;
  kind: "video" | "audio";
  parked?: boolean;
  onError: (message: string) => void;
}) {
  const local = source !== "gdrive";
  const playerRef = useRef<HTMLDivElement>(null);
  const surfaceRef = useRef<HTMLDivElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const audioRef = useRef<HTMLAudioElement>(null);
  const tokenRef = useRef(`vlc-${Math.random().toString(36).slice(2)}`);
  const localPathRef = useRef(path);
  const hideTimer = useRef<number>(0);
  const clickTimer = useRef<number>(0);
  const loopRef = useRef(false);
  const mediaEl = (): HTMLMediaElement | null => videoRef.current ?? audioRef.current;

  const [engine, setEngine] = useState<"vlc" | "html5" | null>(kind === "audio" ? "html5" : null);
  const [src, setSrc] = useState("");
  const [status, setStatus] = useState(kind === "video" ? "Opening in VLC…" : "Loading…");
  const [ready, setReady] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [duration, setDuration] = useState(0);
  const [buffered, setBuffered] = useState(0);
  const [rate, setRate] = useState(1);
  const [volume, setVolume] = useState(1);
  const [muted, setMuted] = useState(false);
  const [loop, setLoop] = useState(false);
  const [fit, setFit] = useState<"contain" | "cover">("contain");
  const [hud, setHud] = useState(true);
  const [fullscreen, setFullscreen] = useState(false);
  const fullscreenRef = useRef(false);
  const [menu, setMenu] = useState<"rate" | "captions" | null>(null);
  const [tracks, setTracks] = useState<SubtitleTrack[]>([]);
  const [activeTrack, setActiveTrack] = useState<string | null>(null);
  const [trackSrc, setTrackSrc] = useState("");
  const [cueSize, setCueSize] = useState(1);

  loopRef.current = loop;

  const surfaceBox = () => {
    const rect = surfaceRef.current?.getBoundingClientRect();
    return rect ? ([rect.left, rect.top, rect.width, rect.height] as const) : null;
  };

  const openVlc = useCallback(
    async (filePath: string) => {
      const box = surfaceBox();
      if (!box) return;
      setEngine("vlc");
      setStatus("");
      await api.vlcOpen(tokenRef.current, filePath, ...box);
      setReady(true);
    },
    [],
  );

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const localPath = source === "gdrive" ? await api.cacheDrive(path, title) : path;
        if (cancelled) return;
        localPathRef.current = localPath;
        if (kind === "audio") {
          setEngine("html5");
          setSrc(fileUrl(localPath));
          setStatus("");
          return;
        }
        const info = await api.vlcAvailable();
        if (cancelled) return;
        if (info.available) {
          await openVlc(localPath);
          const sidecars = await api.listSubtitles(localPath).catch(() => []);
          if (cancelled) return;
          const files = sidecars.filter((t) => t.kind === "sidecar");
          setTracks(files);
          const sidecar = files[0];
          if (sidecar) {
            setActiveTrack(sidecar.id);
            await api.vlcSetSubtitle(sidecar.id).catch(() => undefined);
          }
          return;
        }
        if (playsInWebview(ext)) {
          setEngine("html5");
          setSrc(fileUrl(localPath));
          setStatus(info.message);
          return;
        }
        setEngine(null);
        setStatus(info.message);
        onError(info.message);
      } catch (e) {
        if (cancelled) return;
        if (kind === "video" && playsInWebview(ext)) {
          setEngine("html5");
          setSrc(fileUrl(localPathRef.current));
          setStatus(String(e));
          return;
        }
        onError(String(e));
      }
    })();
    return () => {
      cancelled = true;
      if (kind === "video") {
        void api.vlcClose(tokenRef.current).catch(() => undefined);
      }
    };
  }, [path, source, title, kind, ext, openVlc, onError]);

  useEffect(() => {
    if (engine !== "vlc") return;
    const el = surfaceRef.current;
    if (!el) return;
    const push = () => {
      const b = surfaceBox();
      if (!b) return;
      if (parked) {
        void api.vlcHide().catch(() => undefined);
        return;
      }
      void api.vlcBounds(...b).catch(() => undefined);
    };
    if (parked) {
      void api.vlcHide().catch(() => undefined);
    } else {
      push();
    }
    const observer = new ResizeObserver(push);
    observer.observe(el);
    window.addEventListener("resize", push);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", push);
    };
  }, [engine, parked, fullscreen]);

  useEffect(() => {
    if (engine !== "vlc" || parked) return;
    const id = window.setInterval(() => {
      void api
        .vlcStatus()
        .then(async (st) => {
          if (st.token && st.token !== tokenRef.current) return;
          setPlaying(st.playing);
          setElapsed(st.timeMs / 1000);
          setDuration(st.lengthMs / 1000);
          if (st.lengthMs > 0) setReady(true);
          if (st.ended && loopRef.current) {
            await api.vlcSeek(0);
            await api.vlcPlay();
          }
        })
        .catch(() => undefined);
    }, 250);
    const tracksId = window.setTimeout(() => {
      void api
        .vlcTracks()
        .then((embedded) => {
          if (!embedded.length) return;
          setTracks((cur) => {
            const files = cur.filter((t) => t.kind === "sidecar");
            const seen = new Set(files.map((t) => t.id));
            return [...files, ...embedded.filter((t) => !seen.has(t.id))];
          });
        })
        .catch(() => undefined);
    }, 800);
    return () => {
      window.clearInterval(id);
      window.clearTimeout(tracksId);
    };
  }, [engine, parked]);

  useEffect(() => {
    if (engine !== "html5") return;
    const el = mediaEl();
    if (el) el.playbackRate = rate;
  }, [rate, src, engine]);

  useEffect(() => {
    if (engine !== "html5") return;
    const el = mediaEl();
    if (el) {
      el.volume = volume;
      el.muted = muted;
      el.loop = loop;
    }
  }, [volume, muted, loop, src, engine]);

  useEffect(() => {
    if (engine !== "vlc") return;
    void api.vlcSetRate(rate).catch(() => undefined);
  }, [rate, engine]);

  useEffect(() => {
    if (engine !== "vlc") return;
    void api.vlcSetVolume(volume).catch(() => undefined);
    void api.vlcSetMute(muted).catch(() => undefined);
  }, [volume, muted, engine]);

  useEffect(() => {
    playerRef.current?.focus();
  }, [src, engine]);

  useEffect(() => {
    const v = videoRef.current;
    if (!v) return;
    for (const t of Array.from(v.textTracks)) {
      t.mode = activeTrack ? "showing" : "disabled";
    }
  }, [trackSrc, activeTrack, ready]);

  useEffect(() => {
    return () => {
      fullscreenRef.current = false;
      void getCurrentWindow().setFullscreen(false);
    };
  }, []);

  const pokeHud = () => {
    setHud(true);
    window.clearTimeout(hideTimer.current);
    if (playing && engine !== "vlc") {
      hideTimer.current = window.setTimeout(() => {
        setHud(false);
        setMenu(null);
      }, 2400);
    }
  };

  useEffect(() => {
    pokeHud();
    return () => window.clearTimeout(hideTimer.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playing]);

  const toggle = () => {
    if (engine === "vlc") {
      void api.vlcToggle().catch((e) => onError(String(e)));
      return;
    }
    const el = mediaEl();
    if (!el || !src) return;
    if (el.paused) void el.play();
    else el.pause();
  };

  const seekTo = (ratio: number) => {
    const next = Math.max(0, Math.min(1, ratio)) * duration;
    if (engine === "vlc") {
      void api.vlcSeek(next * 1000).catch(() => undefined);
      setElapsed(next);
      return;
    }
    const el = mediaEl();
    if (!el || !duration) return;
    el.currentTime = next;
  };

  const onScrub = (e: ReactMouseEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    seekTo((e.clientX - rect.left) / rect.width);
  };

  const bump = (delta: number) => {
    const next = Math.max(0, Math.min(duration, elapsed + delta));
    if (engine === "vlc") {
      void api.vlcSeek(next * 1000).catch(() => undefined);
      setElapsed(next);
      return;
    }
    const el = mediaEl();
    if (el) el.currentTime = next;
  };

  const enterFullscreen = async () => {
    const next = !fullscreenRef.current;
    fullscreenRef.current = next;
    setFullscreen(next);
    setHud(true);
    try {
      await getCurrentWindow().setFullscreen(next);
    } catch {
      /* WKWebView has no element.requestFullscreen; covering the window is enough. */
    }
  };

  const enterPip = async () => {
    const el = videoRef.current;
    if (!el) return;
    try {
      if (document.pictureInPictureElement) await document.exitPictureInPicture();
      else await el.requestPictureInPicture();
    } catch (e) {
      onError(String(e));
    }
  };

  const pickTrack = async (id: string | null) => {
    setMenu(null);
    setActiveTrack(id);
    if (engine === "vlc") {
      try {
        await api.vlcSetSubtitle(id);
      } catch (e) {
        onError(String(e));
      }
      return;
    }
    if (!id) {
      setTrackSrc("");
      return;
    }
    try {
      const filePath = source === "gdrive" ? await api.cacheDrive(path, title) : path;
      const vtt = await api.subtitleVtt(filePath, id);
      setTrackSrc(fileUrl(vtt));
    } catch (e) {
      onError(String(e));
    }
  };

  const onKey = (e: ReactKeyboardEvent) => {
    const typing = (e.target as HTMLElement).tagName === "INPUT";
    if (typing) return;
    switch (e.key) {
      case " ":
      case "k":
        e.preventDefault();
        toggle();
        break;
      case "ArrowLeft":
        e.preventDefault();
        bump(-10);
        break;
      case "ArrowRight":
        e.preventDefault();
        bump(10);
        break;
      case "ArrowUp":
        e.preventDefault();
        setVolume((v) => Math.min(1, v + 0.05));
        setMuted(false);
        break;
      case "ArrowDown":
        e.preventDefault();
        setVolume((v) => Math.max(0, v - 0.05));
        break;
      case "m":
        setMuted((m) => !m);
        break;
      case "l":
        setLoop((v) => !v);
        break;
      case "c":
        setMenu((m) => (m === "captions" ? null : "captions"));
        break;
      case "f":
        if (e.metaKey || e.ctrlKey) break;
        e.preventDefault();
        e.stopPropagation();
        void enterFullscreen();
        break;
      case "Escape":
        setMenu(null);
        if (fullscreenRef.current) {
          e.preventDefault();
          e.stopPropagation();
          void enterFullscreen();
        }
        break;
      default:
        break;
    }
  };

  const onSurfaceClick = (e: ReactMouseEvent) => {
    if (kind !== "video" || engine === "vlc") return;
    if (e.detail > 1) {
      window.clearTimeout(clickTimer.current);
      return;
    }
    clickTimer.current = window.setTimeout(() => toggle(), 220);
  };

  const progress = duration ? (elapsed / duration) * 100 : 0;
  const buf = duration ? (buffered / duration) * 100 : 0;
  const vlc = engine === "vlc";

  return (
    <div
      ref={playerRef}
      className={`player${hud || !playing || vlc ? " show-hud" : ""}${fullscreen ? " is-full" : ""}${vlc ? " vlc-engine" : ""}`}
      tabIndex={0}
      onMouseMove={pokeHud}
      onMouseLeave={() => playing && !vlc && setHud(false)}
      onKeyDown={onKey}
    >
      <div
        ref={surfaceRef}
        className={`screen fit-${fit}`}
        onClick={onSurfaceClick}
        onDoubleClick={() => kind === "video" && engine !== "vlc" && void enterFullscreen()}
      >
        {kind === "video" && engine === "html5" && src ? (
          <video
            ref={videoRef}
            src={src}
            playsInline
            onPlay={() => setPlaying(true)}
            onPause={() => setPlaying(false)}
            onTimeUpdate={(e) => {
              setElapsed(e.currentTarget.currentTime);
              const b = e.currentTarget.buffered;
              if (b.length) setBuffered(b.end(b.length - 1));
            }}
            onLoadedMetadata={(e) => {
              setDuration(e.currentTarget.duration);
              setReady(true);
            }}
            onError={() => onError(`Cannot decode ${ext.toUpperCase()} in this webview.`)}
            onWaiting={() => setStatus("Buffering…")}
            onPlaying={() => setStatus("")}
          >
            {trackSrc && (
              <track
                key={trackSrc}
                kind="subtitles"
                src={trackSrc}
                srcLang={tracks.find((t) => t.id === activeTrack)?.language || "und"}
                label={tracks.find((t) => t.id === activeTrack)?.label || "Subtitles"}
                default
              />
            )}
          </video>
        ) : kind !== "video" ? (
          <>
            <span className="blob" />
            <div className="caption">
              <div className="heading">{title}</div>
              <div className="sub">{status || `${ext.toUpperCase()} ${kind}`}</div>
            </div>
            {kind === "audio" && src && (
              <audio
                ref={audioRef}
                src={src}
                onPlay={() => setPlaying(true)}
                onPause={() => setPlaying(false)}
                onTimeUpdate={(e) => setElapsed(e.currentTarget.currentTime)}
                onLoadedMetadata={(e) => {
                  setDuration(e.currentTarget.duration);
                  setReady(true);
                }}
              />
            )}
          </>
        ) : null}
        {kind === "video" && engine === "html5" && src && !playing && (
          <button type="button" className="big-play" onClick={toggle} aria-label="Play">
            <Icon name="play" size={28} />
          </button>
        )}
        {kind === "video" && !ready && <div className="player-status">{status || "Loading…"}</div>}
        {kind === "video" && ready && status && <div className="player-status faint">{status}</div>}
      </div>

      <div className="hud" onClick={(e) => e.stopPropagation()}>
        <div className="hud-top">
          <div className="hud-title">{title}</div>
          <div className="spacer" />
          {kind === "video" && engine !== "vlc" && (
            <button
              className="btn btn-ghost hud-btn"
              onClick={() => setFit((f) => (f === "contain" ? "cover" : "contain"))}
            >
              {fit === "contain" ? "Fit" : "Fill"}
            </button>
          )}
        </div>
        <div className="hud-bottom">
          <div className="scrub-row">
            <span className="time">{formatDuration(elapsed)}</span>
            <div className="progress click" onClick={onScrub}>
              <b style={{ width: `${buf}%` }} />
              <i style={{ width: `${progress}%` }} />
            </div>
            <span className="time">{formatDuration(duration)}</span>
          </div>
          <div className="btn-row hud-row">
            <HudIcon name={playing ? "pause" : "play"} label={playing ? "Pause" : "Play"} onClick={toggle} />
            <HudIcon name="skipBack" label="Back 10 seconds" onClick={() => bump(-10)} />
            <HudIcon name="skipFwd" label="Forward 10 seconds" onClick={() => bump(10)} />
            <div className="vol">
              <HudIcon
                name={muted || volume === 0 ? "volumeOff" : "volume"}
                label="Mute"
                onClick={() => setMuted((m) => !m)}
              />
              <input
                type="range"
                min={0}
                max={1}
                step={0.01}
                value={muted ? 0 : volume}
                onChange={(e) => {
                  const v = Number(e.target.value);
                  setVolume(v);
                  setMuted(v === 0);
                }}
                aria-label="Volume"
              />
            </div>
            <div className="spacer" />
            <div className="hud-pop">
              <button className="btn btn-ghost hud-btn" onClick={() => setMenu((m) => (m === "rate" ? null : "rate"))}>
                {rate === 1 ? "1×" : `${rate}×`}
              </button>
              {menu === "rate" && (
                <div className="hud-menu">
                  {RATES.map((r) => (
                    <button
                      key={r}
                      className={r === rate ? "on" : ""}
                      onClick={() => {
                        setRate(r);
                        setMenu(null);
                      }}
                    >
                      {r}×
                    </button>
                  ))}
                </div>
              )}
            </div>
            {kind === "video" && (
              <div className="hud-pop">
                <HudIcon
                  name="captions"
                  label="Subtitles"
                  onClick={() => setMenu((m) => (m === "captions" ? null : "captions"))}
                  active={Boolean(activeTrack)}
                />
                {menu === "captions" && (
                  <div className="hud-menu">
                    <button className={!activeTrack ? "on" : ""} onClick={() => void pickTrack(null)}>
                      Off
                    </button>
                    {tracks.length === 0 && <div className="hud-empty">No subtitle files found</div>}
                    {tracks.map((t) => (
                      <button key={t.id} className={t.id === activeTrack ? "on" : ""} onClick={() => void pickTrack(t.id)}>
                        {t.label}
                      </button>
                    ))}
                    {engine === "html5" && (
                      <>
                        <div className="hud-split">Size</div>
                        {[0.8, 1, 1.25, 1.5].map((n) => (
                          <button
                            key={n}
                            className={cueSize === n ? "on" : ""}
                            onClick={() => {
                              setCueSize(n);
                              playerRef.current?.style.setProperty("--cue-size", `${n}`);
                            }}
                          >
                            {n}×
                          </button>
                        ))}
                      </>
                    )}
                  </div>
                )}
              </div>
            )}
            <HudIcon name="loop" label="Loop" onClick={() => setLoop((v) => !v)} active={loop} />
            {kind === "video" && engine === "html5" && (
              <HudIcon name="pip" label="Picture in picture" onClick={() => void enterPip()} />
            )}
            {local && (
              <button className="btn btn-ghost hud-btn" onClick={() => void api.openSystem(path).catch((e) => onError(String(e)))}>
                System
              </button>
            )}
            {kind === "video" && (
              <HudIcon
                name={fullscreen ? "collapse" : "expand"}
                label={fullscreen ? "Exit full screen" : "Full screen"}
                onClick={() => void enterFullscreen()}
              />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function HudIcon({
  name,
  label,
  onClick,
  active,
}: {
  name: IconName;
  label: string;
  onClick: () => void;
  active?: boolean;
}) {
  return (
    <button type="button" className={`btn btn-icon hud-icon${active ? " on" : ""}`} aria-label={label} title={label} onClick={onClick}>
      <Icon name={name} size={16} />
    </button>
  );
}
