import { Screen, ScreenHeader } from "../components/Screen";
import { EmptyState } from "../components/states";

export function ZoteroScreen() {
  return (
    <Screen>
      <ScreenHeader
        title="Zotero"
        description="Marginalia reads your Zotero library. Zotero stays the source of truth for everything bibliographic."
      />
      <EmptyState
        title="Zotero is not connected"
        description="Marginalia can read your local Zotero database, or connect to the Zotero API. Either way, it opens your library read-only and never writes to it without you asking. Connecting is not implemented yet — it arrives in Phase 1."
      />
    </Screen>
  );
}
