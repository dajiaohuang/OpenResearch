import type { createDialogs } from "./dialog";
import { m } from "./paraglide/messages.js";

export function confirmArtifactCopy(show: ReturnType<typeof createDialogs>["showConfirmDialog"], copy: () => Promise<void>) {
  show({
    title: m.worktree_copy_to_artifacts(),
    description: m.worktree_copy_changes_warning(),
    confirmLabel: m.worktree_copy_to_artifacts(),
    onCancel: "close",
    onConfirm: copy,
  });
}
