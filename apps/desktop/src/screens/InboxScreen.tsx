/**
 * The Annotation Inbox — every highlight and note from every document, in one
 * place, so the user never has to remember which paper an idea was in.
 */
import { Screen, ScreenHeader } from "../components/Screen";
import { EmptyState } from "../components/states";

export function InboxScreen() {
  return (
    <Screen>
      <ScreenHeader
        title="Annotation Inbox"
        description="Every highlight and note you have made, across every document."
      />
      <EmptyState
        title="Nothing here yet"
        description="Once you read and annotate a document on your reMarkable, your highlights and notes will appear here — each one linked back to its page, so you can always return to the source."
      />
    </Screen>
  );
}
