// 설정 화면 — M1 기본(계정·연결 상태 표시, 연결 끊기, 설치 안내 재열람).
// 토큰 원문은 절대 표시하지 않는다(E2E-2 — 키체인 보관만).
import { useEffect, useState } from "react";
import { ui } from "../uiStrings";

export interface UpdateInfo {
  has_update: boolean;
  notice: string;
  download_url: string;
}

interface SettingsBridge {
  accountDisplay(): Promise<string>;
  connectedRepo(): Promise<string>;
  appVersion(): Promise<string>;
  disconnectAccount(): Promise<void>;
  checkUpdate?(): Promise<UpdateInfo>;
}

declare global {
  interface Window { __aiNoteSettings?: SettingsBridge }
}

export function SettingsScreen({ bridge }: { bridge?: SettingsBridge }) {
  const b = bridge ?? window.__aiNoteSettings;
  const [display, setDisplay] = useState("");
  const [repo, setRepo] = useState("");
  const [version, setVersion] = useState("");
  const [disconnected, setDisconnected] = useState(false);
  const [update, setUpdate] = useState<UpdateInfo | null>(null);

  useEffect(() => {
    b?.checkUpdate?.().then(setUpdate).catch(() => setUpdate(null));
  }, [b]);

  useEffect(() => {
    if (!b) return;
    (async () => {
      setDisplay(await b.accountDisplay());
      setRepo(await b.connectedRepo());
      setVersion(await b.appVersion());
    })();
    b.checkUpdate?.().then(setUpdate).catch(() => setUpdate(null));
  }, [b]);

  async function disconnect() {
    if (!b) return;
    await b.disconnectAccount();
    setDisconnected(true);
    setDisplay("");
    setRepo("");
  }

  return (
    <section aria-label={ui.nav.settings}>
      <h2>{ui.settings.title}</h2>
      <dl>
        <dt>{ui.settings.accountLabel}</dt>
        <dd data-testid="settings-display">{display || (disconnected ? ui.settings.disconnected : "—")}</dd>
        <dt>{ui.settings.vaultLabel}</dt>
        <dd data-testid="settings-repo">{repo || "—"}</dd>
        <dt>{ui.settings.versionLabel}</dt>
        <dd>{version}</dd>
      </dl>
      <button data-testid="disconnect-btn" onClick={disconnect} disabled={!display}>
        {ui.settings.disconnect}
      </button>
      <p>{ui.settings.disconnectHelp}</p>
      {update && (
        <section data-testid="update-section">
          <h3>{ui.settings.updateTitle}</h3>
          <p role="status" data-testid="update-notice">{update.notice}</p>
          {update.has_update && update.download_url && (
            <a
              data-testid="update-download"
              href={update.download_url}
              target="_blank"
              rel="noreferrer"
            >
              {ui.settings.updateDownload}
            </a>
          )}
        </section>
      )}
    </section>
  );
}
