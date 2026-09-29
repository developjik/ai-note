import { ui } from "../uiStrings";

export function HistoryScreen() {
  return (
    <section aria-label={ui.history.title}>
      <span>{ui.history.applied}</span>
      <span>{ui.history.rejected}</span>
    </section>
  );
}
