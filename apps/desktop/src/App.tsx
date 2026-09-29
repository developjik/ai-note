import { useState } from "react";
import { ui } from "./uiStrings";
import { WorkspaceScreen } from "./screens/WorkspaceScreen";
import { ReviewScreen } from "./screens/ReviewScreen";
import { HistoryScreen } from "./screens/HistoryScreen";
import { SettingsScreen } from "./screens/SettingsScreen";
import { OnboardScreen } from "./screens/OnboardScreen";

type ScreenId = "workspace" | "review" | "history" | "settings" | "onboard";

export default function App() {
  const [screen, setScreen] = useState<ScreenId>("workspace");
  const nav: Array<[ScreenId, string]> = [
    ["workspace", ui.nav.workspace],
    ["review", ui.nav.review],
    ["history", ui.nav.history],
    ["settings", ui.nav.settings],
  ];
  return (
    <div className="app">
      <nav>
        {nav.map(([id, label]) => (
          <button key={id} aria-current={screen === id} onClick={() => setScreen(id)}>
            {label}
          </button>
        ))}
      </nav>
      <main>
        {screen === "workspace" && <WorkspaceScreen />}
        {screen === "review" && <ReviewScreen />}
        {screen === "history" && <HistoryScreen />}
        {screen === "settings" && <SettingsScreen />}
        {screen === "onboard" && <OnboardScreen />}
      </main>
    </div>
  );
}
