import { useState } from "react";
import type { AppSettings, DiskUsage, DriveAccount, Place, SocialAppKind } from "../types";
import { formatBytes } from "../lib/files";
import { Icon, type IconName } from "../lib/icons";

interface ConnectionsProps {
  accounts: DriveAccount[];
  driveQuota: Record<string, DiskUsage>;
  places: Place[];
  settings: AppSettings;
  volumeUsage: Record<string, DiskUsage>;
  onConfigure: () => void;
  onConnectGoogle: () => Promise<void>;
  onDisconnectGoogle: (accountId: string) => Promise<void>;
  onOpenGoogle: (account: DriveAccount) => void;
  onOpenLocal: (place: Place) => void;
  onOpenWeb: (url: string) => void;
  onOpenSocial: (app: SocialAppKind) => void;
  onError: (message: string) => void;
}

function ProviderMark({ label, tone }: { label: string; tone: string }) {
  return <span className={`provider-mark ${tone}`}>{label}</span>;
}

export function ProviderHeading({
  mark,
  tone,
  title,
  icon,
  status,
}: {
  mark?: string;
  tone?: string;
  title: string;
  icon?: IconName;
  status?: string;
}) {
  return (
    <div className="provider-heading">
      {mark ? <ProviderMark label={mark} tone={tone || "neutral"} /> : <span className="provider-mark neutral"><Icon name={icon || "cloud"} size={19} /></span>}
      <div>
        <div className="provider-title">{title}</div>
        {status && <div className="provider-status">{status}</div>}
      </div>
    </div>
  );
}

export function Connections({
  accounts,
  driveQuota,
  places,
  settings,
  volumeUsage,
  onConfigure,
  onConnectGoogle,
  onDisconnectGoogle,
  onOpenGoogle,
  onOpenLocal,
  onOpenWeb,
  onOpenSocial,
  onError,
}: ConnectionsProps) {
  const [connecting, setConnecting] = useState(false);
  const googleReady = Boolean(settings.googleClientId.trim() && settings.googleClientSecret.trim());
  const oneDriveReady = Boolean(settings.oneDriveClientId.trim() && settings.oneDriveClientSecret.trim());
  const dropboxReady = Boolean(settings.dropboxClientId.trim() && settings.dropboxClientSecret.trim());
  const s3Ready = Boolean(
    settings.s3Region.trim() &&
      settings.s3Bucket.trim() &&
      settings.s3AccessKeyId.trim() &&
      settings.s3SecretAccessKey.trim(),
  );

  const connectGoogle = async () => {
    setConnecting(true);
    try {
      await onConnectGoogle();
    } catch (error) {
      onError(String(error));
    } finally {
      setConnecting(false);
    }
  };

  return (
    <div className="connections-page">
      <div className="connections-hero">
        <div>
          <div className="heading">Connections</div>
          <p>Keep several Google accounts beside web access to your other storage and social apps.</p>
        </div>
        <button className="btn btn-secondary" onClick={onConfigure}>
          Provider settings
        </button>
      </div>

      <section className="connection-section">
        <div className="connection-section-title">
          <div>
            <h2>Cloud storage</h2>
            <p>Google Drive is connected to Depot's file browser. Other providers open securely in native web tabs while their API adapters are prepared.</p>
          </div>
          <span className="tag tag-accent-2">{accounts.length} Google account{accounts.length === 1 ? "" : "s"}</span>
        </div>

        <div className="provider-grid">
          <article className="provider-card provider-card-wide">
            <ProviderHeading
              mark="G"
              tone="google"
              title="Google Drive"
              status={googleReady ? "OAuth ready" : "Add OAuth credentials"}
            />
            <p className="provider-copy">Browse, preview, and copy files between Google accounts and your computer. Sign in again to add another Google account; reconnecting the same email refreshes it without creating a duplicate.</p>
            <div className="provider-actions">
              <button className="btn btn-primary" disabled={!googleReady || connecting} onClick={() => void connectGoogle()}>
                {connecting ? "Waiting for Google…" : "Sign in with Google"}
              </button>
              {!googleReady && <button className="btn btn-secondary" onClick={onConfigure}>Add client ID</button>}
            </div>
            <div className="account-list">
              {!accounts.length && <div className="account-empty">No Google accounts connected yet.</div>}
              {accounts.map((account) => {
                const quota = driveQuota[account.id];
                const used = quota ? quota.total - quota.free : 0;
                const percent = quota?.total ? Math.round((used / quota.total) * 100) : 0;
                return (
                  <div className="account-row" key={account.id}>
                    <ProviderMark label={account.email.slice(0, 1).toUpperCase()} tone="account" />
                    <div className="account-details">
                      <strong>{account.email}</strong>
                      <span>{quota ? `${formatBytes(used)} of ${formatBytes(quota.total)} used` : "Connected"}</span>
                      {quota && <span className="account-meter"><i style={{ width: `${percent}%` }} /></span>}
                    </div>
                    <button className="btn btn-secondary" onClick={() => onOpenGoogle(account)}>Open</button>
                    <button
                      className="btn btn-ghost"
                      onClick={() => void onDisconnectGoogle(account.id).catch((error) => onError(String(error)))}
                    >
                      Disconnect
                    </button>
                  </div>
                );
              })}
            </div>
          </article>

          <article className="provider-card">
            <ProviderHeading mark="M" tone="microsoft" title="OneDrive" status={oneDriveReady ? "Credentials saved" : "Setup available"} />
            <p className="provider-copy">Use OneDrive in a Depot web tab now. Client credentials are stored for the native file-browser adapter.</p>
            <div className="provider-actions">
              <button className="btn btn-primary" onClick={() => onOpenWeb("https://onedrive.live.com/")}>Open & sign in</button>
              <button className="btn btn-secondary" onClick={onConfigure}>{oneDriveReady ? "Edit API setup" : "Configure API"}</button>
            </div>
          </article>

          <article className="provider-card">
            <ProviderHeading mark="D" tone="dropbox" title="Dropbox" status={dropboxReady ? "Credentials saved" : "Setup available"} />
            <p className="provider-copy">Open the Dropbox website inside Depot. OAuth values can be added now without committing secrets to the repo.</p>
            <div className="provider-actions">
              <button className="btn btn-primary" onClick={() => onOpenWeb("https://www.dropbox.com/home")}>Open & sign in</button>
              <button className="btn btn-secondary" onClick={onConfigure}>{dropboxReady ? "Edit API setup" : "Configure API"}</button>
            </div>
          </article>

          <article className="provider-card">
            <ProviderHeading icon="bucket" title="Amazon S3" status={s3Ready ? "Profile saved" : "Access-key setup"} />
            <p className="provider-copy">S3 uses a bucket, region, and access-key profile instead of browser OAuth. A custom endpoint supports compatible services.</p>
            <div className="provider-actions">
              <button className="btn btn-secondary" onClick={onConfigure}>{s3Ready ? "Edit S3 profile" : "Configure S3"}</button>
            </div>
          </article>
        </div>
      </section>

      <section className="connection-section">
        <div className="connection-section-title">
          <div>
            <h2>Social apps</h2>
            <p>These open as dedicated Depot apps with their own tabs and persistent system-webview sessions. Depot never sees your password.</p>
          </div>
        </div>
        <div className="provider-grid social-grid">
          <article className="provider-card social-card">
            <ProviderHeading mark="f" tone="facebook" title="Facebook" status="Depot app" />
            <p className="provider-copy">Sign in once and keep Facebook available from the Social apps category.</p>
            <button className="btn btn-primary" onClick={() => onOpenSocial("facebook")}>Open Facebook app</button>
          </article>
          <article className="provider-card social-card">
            <ProviderHeading mark="◎" tone="instagram" title="Instagram" status="Depot app" />
            <p className="provider-copy">Sign in once and keep Instagram available from the Social apps category.</p>
            <button className="btn btn-primary" onClick={() => onOpenSocial("instagram")}>Open Instagram app</button>
          </article>
        </div>
      </section>

      <section className="connection-section">
        <div className="connection-section-title">
          <div>
            <h2>Local storage</h2>
            <p>Mounted disks and your home volume remain available without an account.</p>
          </div>
        </div>
        <div className="local-storage-grid">
          {places.map((place) => {
            const usage = volumeUsage[place.path];
            const used = usage ? usage.total - usage.free : 0;
            const percent = usage?.total ? Math.round((used / usage.total) * 100) : 0;
            return (
              <button className="local-storage-card" key={place.path} onClick={() => onOpenLocal(place)}>
                <span className="provider-mark neutral"><Icon name={place.kind === "home" ? "home" : "disk"} size={18} /></span>
                <span className="local-storage-details">
                  <strong>{place.name}</strong>
                  <span>{usage ? `${formatBytes(usage.free)} free of ${formatBytes(usage.total)}` : place.path}</span>
                  {usage && <span className="account-meter"><i style={{ width: `${percent}%` }} /></span>}
                </span>
                <Icon name="forward" size={16} />
              </button>
            );
          })}
        </div>
      </section>
    </div>
  );
}
