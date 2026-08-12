/**
 * The application shell.
 *
 * Phase 0 ships the frame: navigation, the design language, and the three
 * required states on every screen. The screens are honest about being empty —
 * there is no mock data pretending the product works, because a fake Library
 * is how you end up shipping a Send button that was never wired to a safety
 * check.
 */

import { useState } from "react";
import { Sidebar, type ScreenId } from "./components/Sidebar";
import { LibraryScreen } from "./screens/LibraryScreen";
import { InboxScreen } from "./screens/InboxScreen";
import { DeviceScreen } from "./screens/DeviceScreen";
import { ActivityScreen } from "./screens/ActivityScreen";
import { ZoteroScreen } from "./screens/ZoteroScreen";
import { SettingsScreen } from "./screens/SettingsScreen";

const SCREENS: Record<ScreenId, () => JSX.Element> = {
  library: LibraryScreen,
  inbox: InboxScreen,
  zotero: ZoteroScreen,
  device: DeviceScreen,
  activity: ActivityScreen,
  settings: SettingsScreen,
};

export default function App() {
  const [screen, setScreen] = useState<ScreenId>("library");
  const Screen = SCREENS[screen];

  return (
    <div className="flex h-full">
      <Sidebar current={screen} onNavigate={setScreen} />
      <main className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-4xl px-10 py-10">
          <Screen />
        </div>
      </main>
    </div>
  );
}
