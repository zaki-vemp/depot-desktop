import { useEffect, useState } from "react";
import { api } from "../api";
import type { OfficePreview as OfficePreviewData, SourceKind } from "../types";
import { OpenWithMenu } from "./OpenWith";

export function OfficePreview({
  title,
  path,
  source,
  onError,
}: {
  title: string;
  path: string;
  source: SourceKind;
  onError: (message: string) => void;
}) {
  const [data, setData] = useState<OfficePreviewData | null>(null);
  const [sheet, setSheet] = useState(0);
  const [status, setStatus] = useState("Opening…");
  const local = source !== "gdrive";

  useEffect(() => {
    let cancelled = false;
    setData(null);
    setSheet(0);
    setStatus("Opening…");
    (async () => {
      try {
        const file = source === "gdrive" ? await api.cacheDrive(path, title) : path;
        const preview = await api.previewOffice(file);
        if (cancelled) return;
        setData(preview);
        setStatus("");
      } catch (e) {
        if (!cancelled) {
          setStatus(String(e));
          onError(String(e));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [path, source, title, onError]);

  const filePath = path;
  const rows = data?.sheets[sheet]?.rows ?? [];

  return (
    <div className="doc office">
      <div className="doc-head">
        <div className="heading">{title}</div>
        <div className="spacer" />
        {local && <OpenWithMenu path={filePath} onError={onError} variant="bar" />}
      </div>
      {data?.sheets && data.sheets.length > 1 && (
        <div className="sheet-tabs">
          {data.sheets.map((s, i) => (
            <button key={s.name} className={`sheet-tab${i === sheet ? " on" : ""}`} onClick={() => setSheet(i)}>
              {s.name}
            </button>
          ))}
        </div>
      )}
      <div className="doc-frame office-frame">
        {!data && <div className="empty">{status || "Loading…"}</div>}
        {data?.note && !data.pages.length && !data.sheets.some((s) => s.rows.length) && (
          <div className="empty">{data.note}</div>
        )}
        {data?.kind === "spreadsheet" && rows.length > 0 && (
          <div className="sheet-wrap">
            <table className="sheet">
              <tbody>
                {rows.map((row, ri) => (
                  <tr key={ri}>
                    <th>{ri + 1}</th>
                    {row.map((cell, ci) =>
                      ri === 0 ? <th key={ci}>{cell}</th> : <td key={ci}>{cell}</td>,
                    )}
                  </tr>
                ))}
              </tbody>
            </table>
            {data.truncated && <div className="sheet-note">Showing the first rows — open in another app for the full workbook.</div>}
          </div>
        )}
        {(data?.kind === "document" || data?.kind === "slides") &&
          data.pages.map((page) => (
            <article className="office-page" key={page.title}>
              <h2>{page.title}</h2>
              <pre>{page.body}</pre>
            </article>
          ))}
      </div>
    </div>
  );
}
