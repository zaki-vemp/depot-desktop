import { useEffect, useState } from "react";
import { api } from "../api";
import type { OpenApp } from "../types";

export function OpenWithMenu({
  path,
  onError,
  onPicked,
  variant = "menu",
}: {
  path: string;
  onError: (message: string) => void;
  onPicked?: () => void;
  variant?: "menu" | "bar";
}) {
  const [apps, setApps] = useState<OpenApp[]>([]);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (!path) return;
    void api.listOpenWith(path).then(setApps).catch(() => setApps([]));
  }, [path]);

  const run = async (app: string) => {
    try {
      if (app === "__pick__") await api.pickOpenWith(path);
      else await api.openWithApp(path, app);
      onPicked?.();
    } catch (e) {
      onError(String(e));
    }
  };

  if (variant === "bar") {
    return (
      <div className="openwith-bar">
        <button className="btn btn-secondary" onClick={() => void api.openSystem(path).catch((e) => onError(String(e)))}>
          Default app
        </button>
        <div className="hud-pop">
          <button className="btn btn-secondary" onClick={() => setOpen((v) => !v)}>
            Open with
          </button>
          {open && (
            <div className="hud-menu openwith-menu">
              {apps.map((a) => (
                <button
                  key={a.path}
                  onClick={() => {
                    setOpen(false);
                    void run(a.path);
                  }}
                >
                  {a.name}
                  {a.isDefault ? " · Default" : ""}
                </button>
              ))}
              <button
                onClick={() => {
                  setOpen(false);
                  void run("__pick__");
                }}
              >
                Other…
              </button>
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className={`ctx-sub${open ? " open" : ""}`}>
      <button
        type="button"
        onMouseEnter={() => setOpen(true)}
        onClick={() => setOpen((v) => !v)}
      >
        Open with
      </button>
      <div className="ctx-fly">
        <button
          onClick={() => {
            onPicked?.();
            void api.openSystem(path).catch((e) => onError(String(e)));
          }}
        >
          Default app
        </button>
        {apps.map((a) => (
          <button key={a.path} onClick={() => void run(a.path)}>
            {a.name}
            {a.isDefault ? " · Default" : ""}
          </button>
        ))}
        <button onClick={() => void run("__pick__")}>Other…</button>
      </div>
    </div>
  );
}
