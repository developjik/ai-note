import { ui } from "../uiStrings";

export function WorkspaceScreen() {
  return (
    <section aria-label={ui.nav.workspace}>
      <input placeholder={ui.workspace.searchPlaceholder} />
      <button>{ui.workspace.newDocument}</button>
    </section>
  );
}
