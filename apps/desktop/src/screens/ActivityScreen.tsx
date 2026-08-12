/**
 * The journal. Every sync, transfer and safety decision, in plain language,
 * with technical detail on demand.
 */
import { Screen, ScreenHeader } from "../components/Screen";
import { EmptyState } from "../components/states";

export function ActivityScreen() {
  return (
    <Screen>
      <ScreenHeader
        title="Activity"
        description="What Marginalia has done, including every decision it made about your device."
      />
      <EmptyState
        title="No activity yet"
        description="Syncs, transfers and safety decisions are recorded here — including the ones Marginalia refused to make, and why."
      />
    </Screen>
  );
}
