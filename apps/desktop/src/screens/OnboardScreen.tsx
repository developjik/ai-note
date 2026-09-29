import { ui } from "../uiStrings";

export function OnboardScreen() {
  return (
    <section aria-label={ui.onboard.title}>
      <h2>{ui.onboard.claudeInstall}</h2>
      <p>{ui.onboard.invite}</p>
    </section>
  );
}
