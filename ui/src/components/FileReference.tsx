import { WorkspaceDomContext } from "../WorkspaceDom";
import { useWorkspace } from "../workspace";
import { DirectionProvider } from "@base-ui/react/direction-provider";
import { Menu } from "@base-ui/react/menu";
import { useQuery } from "@tanstack/react-query";
import { ChevronDown, FileCode } from "lucide-react";
import { createContext, useContext, useMemo, useRef, useState, type ReactNode } from "react";
import { ltr } from "../i18n";
import { GLOB_CHARS, resolveFileReference } from "../fileReferences";
import { splitMentionPath } from "../fileMentions";
import { isWindowsDrivePath, splitLineSuffix } from "../markdownTarget";
import { m } from "../paraglide/messages.js";
import { parseFilePath } from "../fileTabPath";
import { RuntimeContext } from "../RemoteRuntime";

import { tabOpenGestureHandlers, type TabOpenIntent } from "../tabPreview";
import { MenuItem, showAlert } from "./ui";
import { FileContextMenu, fileContextMenuTarget, type FileContextMenuTarget } from "./FileTreeActions";

export type OpenFile = (
  path: string,
  line: number | undefined,
  exp: string | undefined,
  ref: string | undefined,
  intent: TabOpenIntent,
) => void;

/** The checkout a chat's file references are checked against. */
export const FileScopeContext = createContext<{ projectId: string; sessionId?: string | null } | undefined>(undefined);

export function FileChip({ path, line, exp, onOpenFile, children }: {
  path: string;
  line?: number;
  /** Experiment id this file was cited from, if any (`<file exp=…>`). */
  exp?: string;
  onOpenFile?: OpenFile;
  children?: ReactNode;
}) {
  const workspace = useWorkspace();
  const scope = useContext(FileScopeContext);
  const runtime = useContext(RuntimeContext);
  const [menu, setMenu] = useState<FileContextMenuTarget | null>(null);
  const [location, setLocation] = useState<{ path: string; source: "checkout" | "artifact" | "absolute"; sessionId?: string; root?: string; expectedPath: string } | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const sequence = useRef(0);
  const name = path.split("/").pop() || path;
  const label = line != null ? `${name}:${line}` : name;
  const openMenu = (event: React.MouseEvent<HTMLButtonElement>) => {
    if (!scope || !onOpenFile) return;
    event.preventDefault();
    setMenu(fileContextMenuTarget(event, path));
    setLocation(null);
    setNote(null);
    const request = ++sequence.current;
    void (async () => {
      const projects = await workspace.queryClient.fetchQuery(workspace.queries.listProjectsQuery());
      const project = projects.find((item) => item.id === scope.projectId);
      if (!project) throw new Error(m.file_tree_location_unavailable());
      const tab = parseFilePath(path, project.repoPath, scope.sessionId ?? undefined, project.artifactsDir ?? project.filesDir, project.slug);
      if (!tab) throw new Error(m.file_tree_location_unavailable());
      if (exp && (tab.source == null || tab.source === "repo")) throw new Error(m.file_tree_location_historical());
      const loaded = await workspace.queryClient.fetchQuery({ ...workspace.queries.resolvedFileQuery(scope.projectId, tab.path, tab.source ?? "repo", tab.sessionId, tab.ref), staleTime: 0 });
      if (loaded.file.notFound || (loaded.source === "checkout" && loaded.file.root === "branch")) throw new Error(m.file_tree_location_unavailable());
      const selected = { path: loaded.file.path, source: loaded.source, ...(tab.sessionId ? { sessionId: tab.sessionId } : {}), ...(loaded.source === "checkout" ? { root: loaded.file.root } : {}) };
      const current = await workspace.api.fileLocation(scope.projectId, selected);
      if (request !== sequence.current) return;
      setLocation({ ...selected, expectedPath: current.path });
    })().catch((error: unknown) => { if (request === sequence.current) setNote(error instanceof Error ? error.message : String(error)); });
  };
  const action = async (reveal: boolean) => {
    if (!scope || !location) return;
    try {
      const result = await workspace.api.fileLocation(scope.projectId, location, reveal);
      if (!reveal) {
        await navigator.clipboard.writeText(result.path);
        showAlert(m.common_copied(), "success");
      }
    } catch (error) { showAlert(error instanceof Error ? error.message : String(error), "error"); }
  };
  return <>
    <button
      className="file-chip"
      title={onOpenFile ? m.a11y_open_file_in_panel({ path: ltr(path) }) : path}
      {...tabOpenGestureHandlers<HTMLButtonElement>((intent) =>
        onOpenFile?.(path, line, exp, undefined, intent),
      )}
      disabled={!onOpenFile}
      onContextMenu={openMenu}
    >
      {children ?? <><FileCode size={12} /><span className="file-chip-label">{label}</span></>}
    </button>
    {menu && <FileContextMenu target={menu} onOpen={() => onOpenFile?.(path, line, exp, undefined, "preview")}
      onCopyPath={location ? () => void action(false) : undefined}
      onReveal={location && runtime?.kind === "local" ? () => void action(true) : undefined}
      note={note ?? (!location ? m.settings_page_loading() : undefined)}
      onClose={() => { sequence.current++; setMenu(null); }} />}
  </>;
}

/** A reference that is not certain to be one file: the reader picks the match. */
function FileChoices({ label, paths, total, line, onOpenFile }: {
  label: string;
  paths: string[];
  total: number;
  line?: number;
  onOpenFile: OpenFile;
}) {
  const { portalContainer } = useContext(WorkspaceDomContext);

  const title = m.md_pick_file({ path: ltr(label) });
  return (
    <DirectionProvider direction={document.documentElement.dir === "rtl" ? "rtl" : "ltr"}>
      <Menu.Root modal={false}>
        <Menu.Trigger className="file-chip" title={title} aria-label={title}>
          <FileCode size={12} />
          <span className="file-chip-label">{label}</span>
          <ChevronDown size={12} />
        </Menu.Trigger>
        <Menu.Portal container={portalContainer}>
          <Menu.Positioner className="z-50" positionMethod="fixed" align="start" sideOffset={4} collisionPadding={8}>
            <Menu.Popup aria-label={title} className="outline-none max-h-(--available-height) overflow-y-auto min-w-48 max-w-[min(32rem,calc(100vw-2rem))] rounded-lg border border-border bg-background p-1.5 shadow-menu">
              {paths.map((path) => {
                const { name, parent } = splitMentionPath(path);
                return (
                  <Menu.Item key={path} nativeButton render={<MenuItem size="compact" className="text-sm data-highlighted:bg-surface" />}
                    onClick={() => onOpenFile(path, line, undefined, undefined, "preview")}>
                    <span dir="ltr" className="min-w-0 truncate">
                      <span className="font-medium">{name}</span>
                      {parent && <span className="ms-2 text-subtext">{parent}</span>}
                    </span>
                  </Menu.Item>
                );
              })}
              {total > paths.length && (
                <div className="px-2 py-1 text-xs text-subtext">{m.common_more_count({ count: total - paths.length })}</div>
              )}
            </Menu.Popup>
          </Menu.Positioner>
        </Menu.Portal>
      </Menu.Root>
    </DirectionProvider>
  );
}

const isRepoRelative = (path: string) =>
  !path.startsWith("/") && !path.startsWith("~") && !path.startsWith("artifacts/") && !isWindowsDrivePath(path);

/**
 * A reference to a repo file, checked against the chat's checkout so it never
 * looks clickable and fails: an exact path opens, a fuzzy or glob match asks
 * which file, and a path that names nothing renders as `fallback`.
 * The listing omits gitignored files and caps out on huge repos, so a path it
 * lacks is read directly before it is written off.
 * An `optimistic` citation (a `<file>` tag or link) shows as a chip until
 * verification says otherwise; inferred ones (code spans) stay `fallback` until verified.
 */
export function FileReference({ path, line, exp, optimistic, fallback, onOpenFile }: {
  path: string;
  line?: number;
  exp?: string;
  optimistic: boolean;
  fallback: ReactNode;
  onOpenFile: OpenFile;
}) {
  const { getCodeTreeQuery, getProjectFileQuery } = useWorkspace().queries;

  const scope = useContext(FileScopeContext);
  // An experiment's file lives on its own branch, which this listing does not cover.
  const checkable = scope != null && exp == null && isRepoRelative(path);
  const tree = useQuery({
    ...getCodeTreeQuery(scope?.projectId ?? "", scope?.sessionId ? { sessionId: scope.sessionId } : {}),
    // The agent cites files it just wrote; a long-cached listing would miss them.
    staleTime: 5_000,
    enabled: checkable,
  });
  const entries = tree.data?.entries;
  const resolution = useMemo(() => (entries ? resolveFileReference(entries, path) : null), [entries, path]);
  const unlisted = resolution?.kind === "none" && !GLOB_CHARS.test(path);
  const read = useQuery({
    ...getProjectFileQuery(scope?.projectId ?? "", splitLineSuffix(path).path, scope?.sessionId ? { sessionId: scope.sessionId } : {}),
    enabled: unlisted,
  });
  const chip = <FileChip path={path} line={line} exp={exp} onOpenFile={onOpenFile} />;

  if (!checkable || !resolution) return optimistic ? chip : fallback;
  if (resolution.kind === "exact") return <FileChip path={resolution.path} line={resolution.line ?? line} exp={exp} onOpenFile={onOpenFile} />;
  if (resolution.kind === "choices") {
    return <FileChoices label={path} paths={resolution.paths} total={resolution.total} line={resolution.line ?? line} onOpenFile={onOpenFile} />;
  }
  if (read.isSuccess) return chip;
  return optimistic && unlisted && read.isPending ? chip : fallback;
}
