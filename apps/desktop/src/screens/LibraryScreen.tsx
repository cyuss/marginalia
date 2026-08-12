/**
 * The Library.
 *
 * This screen carries the product's central distinction, and it carries it in
 * the copy as much as in the code: syncing brings knowledge, sending moves a
 * file. The empty state teaches that before there is any data to confuse it
 * with.
 */
import { Screen, ScreenHeader } from "../components/Screen";
import { EmptyState } from "../components/states";

export function LibraryScreen() {
  return (
    <Screen>
      <ScreenHeader
        title="Library"
        description="Your Zotero library, with what is on your reMarkable and what you have annotated."
      />
      <EmptyState
        title="No library connected yet"
        description="Connect Zotero to browse your papers here. Marginalia reads your library's metadata — titles, authors, collections, tags, and whether a PDF is available. It never copies a PDF to your reMarkable on its own; that only happens when you press Send to reMarkable on a specific document."
      />
    </Screen>
  );
}
