// 온보딩 마법사 — M1 (E2E-1/2/3: 초대장 → 설치 확인 → 구독 확인 → 연결).
// 상태는 순수 단계 기계(steps.ts)가 소유하고 이 화면은 구독·invoke만.
import { useEffect, useReducer, useState } from "react";
import { ui } from "../uiStrings";
import {
  initialOnboarding,
  reduce,
  isManualGuideMode,
  isSubscriptionGuideMode,
  doneHeadline,
} from "../onboarding/steps";

// 브리지 호출 — tauri API가 없는 vitest 환경을 위해 주입 가능하게.
export interface Bridge {
  detectClaude(): Promise<{ state: "ready" | "outdated" | "notInstalled"; version: string }>;
  claudeManualSteps(): Promise<Array<[string, string]>>;
  subscriptionState(): Promise<"loggedIn" | "needsLogin">;
  subscriptionGuide(): Promise<Array<[string, string]>>;
  connectInvitation(invitation: string, passphrase: string): Promise<{ result: "admin" | "member"; repo: string }>;
}

declare global {
  interface Window { __aiNoteBridge?: Bridge }
}

export function OnboardScreen({ bridge }: { bridge?: Bridge }) {
  const b = bridge ?? window.__aiNoteBridge;
  const [s, dispatch] = useReducer(reduce, initialOnboarding);
  const [invitation, setInvitation] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [steps, setSteps] = useState<Array<[string, string]>>([]);

  // claude 단계 진입 시 감지 1회 + 미설치면 수동 안내(하이브리드 F34)
  useEffect(() => {
    if (s.step !== "claude" || s.claude !== null || !b) return;
    (async () => {
      const d = await b.detectClaude();
      dispatch({ kind: "claude-detected", state: d.state });
      if (d.state !== "ready") {
        setSteps(await b.claudeManualSteps());
        if (d.state === "notInstalled") {
          dispatch({ kind: "auto-install-failed" });
        }
      }
    })();
  }, [s.step, s.claude, b]);

  // 구독 단계 진입 시 감지 1회 + 미로그인이면 안내 문구 확보(Q2)
  useEffect(() => {
    if (s.step !== "subscription" || s.subscription !== null || !b) return;
    (async () => {
      const st = await b.subscriptionState();
      dispatch({ kind: "subscription-detected", state: st });
      if (st === "needsLogin") {
        setSteps(await b.subscriptionGuide());
      }
    })();
  }, [s.step, s.subscription, b]);

  async function recheckSubscription() {
    if (!b) return;
    const st = await b.subscriptionState();
    dispatch({ kind: "subscription-recheck", state: st });
  }

  async function doConnect() {
    if (!b) return;
    dispatch({ kind: "connect-start" });
    try {
      const r = await b.connectInvitation(invitation, passphrase);
      dispatch({ kind: "connect-done", result: r.result });
    } catch (e) {
      dispatch({ kind: "connect-failed", errorKey: String(e) });
    }
  }

  return (
    <main className="onboard">
      <h1>{ui.onboard.title}</h1>
      {s.step === "invite" && (
        <section>
          <p data-testid="unsigned-notice" role="note">
            {ui.onboard.unsignedNotice}
          </p>
          <p>{ui.onboard.inviteHelp}</p>
          <textarea
            data-testid="invite-input"
            value={invitation}
            onChange={(e) => setInvitation(e.target.value)}
            placeholder={ui.onboard.invitePlaceholder}
            rows={4}
          />
          <input
            data-testid="passphrase-input"
            type="password"
            value={passphrase}
            onChange={(e) => setPassphrase(e.target.value)}
            placeholder={ui.onboard.passphrasePlaceholder}
          />
          <button
            data-testid="submit-invite"
            disabled={invitation.trim().length === 0 || passphrase.length === 0}
            onClick={() => dispatch({ kind: "submit-invite" })}
          >
            {ui.onboard.inviteNext}
          </button>
        </section>
      )}
      {s.step === "claude" && (
        <section>
          <p>{ui.onboard.claudeCheck}</p>
          {isManualGuideMode(s) ? (
            <ol data-testid="manual-steps">
              {steps.map(([t, body], i) => (
                <li key={i}>
                  <strong>{t}</strong> — {body}
                </li>
              ))}
            </ol>
          ) : (
            <p data-testid="claude-wait">{ui.onboard.claudeChecking}</p>
          )}
        </section>
      )}
      {s.step === "subscription" && (
        <section>
          {isSubscriptionGuideMode(s) ? (
            <>
              <p data-testid="subscription-guide-title">{ui.onboard.subscriptionGuideTitle}</p>
              <ol data-testid="subscription-steps">
                {steps.map(([t, body], i) => (
                  <li key={i}>
                    <strong>{t}</strong> — {body}
                  </li>
                ))}
              </ol>
              <button data-testid="subscription-recheck" onClick={recheckSubscription}>
                {ui.onboard.subscriptionRecheck}
              </button>
            </>
          ) : (
            <p data-testid="subscription-wait">{ui.onboard.subscriptionChecking}</p>
          )}
        </section>
      )}
      {s.step === "connect" && (
        <section>
          <p>{ui.onboard.connectHelp}</p>
          <button data-testid="connect-btn" disabled={s.connecting} onClick={doConnect}>
            {s.connecting ? ui.onboard.connecting : ui.onboard.connectNow}
          </button>
          {s.errorKey && <p role="alert">{s.errorKey}</p>}
        </section>
      )}
      {s.step === "done" && (
        <section>
          <h2 data-testid="done-headline">{doneHeadline(s)}</h2>
          <p>{ui.onboard.doneHelp}</p>
        </section>
      )}
    </main>
  );
}
