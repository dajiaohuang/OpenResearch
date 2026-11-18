import { ChevronDown, ChevronUp, X } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import { m } from "../paraglide/messages.js";
import { fmtNumber } from "../i18n";
import { EscapeScope } from "./EscapeScope";
import { IconButton, Tooltip } from "./ui";

type Match = { start: number; end: number };
type TextPart = { node: Text; start: number; end: number };

function content(target: HTMLElement, editor: boolean) {
  const textarea = editor ? target.querySelector("textarea") : null;
  if (textarea) return { text: textarea.value, textarea, parts: [] as TextPart[] };
  const root = target.querySelector<HTMLElement>("[data-file-search-content]");
  if (!root) return { text: "", textarea: null, parts: [] as TextPart[] };
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const parts: TextPart[] = [];
  let text = "";
  let previous: Element | null = null;
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const part = node as Text;
    const block = part.parentElement?.closest(".file-view-line, p, li, h1, h2, h3, h4, pre") ?? root;
    if (previous && block !== previous) text += "\n";
    const start = text.length;
    text += part.data;
    parts.push({ node: part, start, end: text.length });
    previous = block;
  }
  return { text, textarea: null, parts };
}

export function FileFind({ target, editor, revision, request, onClose }: {
  target: RefObject<HTMLElement | null>;
  editor: boolean;
  revision: string;
  request: number;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [current, setCurrent] = useState(-1);
  const [snapshot, setSnapshot] = useState<{ text: string; textarea: HTMLTextAreaElement | null; parts: TextPart[] }>({ text: "", textarea: null, parts: [] });
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => { input.current?.focus(); input.current?.select(); }, [request]);
  useLayoutEffect(() => { if (target.current) setSnapshot(content(target.current, editor)); }, [target, editor, revision]);
  const matches = useMemo(() => {
    const found: Match[] = [];
    if (!query) return found;
    const needle = query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    for (const match of snapshot.text.matchAll(new RegExp(needle, "giu"))) {
      found.push({ start: match.index, end: match.index + match[0].length });
    }
    return found;
  }, [snapshot.text, query]);
  const index = matches.length ? Math.max(0, Math.min(current, matches.length - 1)) : 0;
  const move = useCallback((offset: number) => {
    if (!matches.length) return;
    const next = current < 0 ? (offset < 0 ? matches.length - 1 : 0) : (index + offset + matches.length) % matches.length;
    setCurrent(next);
    const match = matches[next];
    if (snapshot.textarea) {
      snapshot.textarea.focus();
      snapshot.textarea.setSelectionRange(match.start, match.end);
      const line = snapshot.text.slice(0, match.start).split("\n").length;
      const row = snapshot.textarea.parentElement?.querySelector<HTMLElement>(`[data-line="${line}"]`);
      if (row) {
        snapshot.textarea.scrollTop = Math.max(0, row.offsetTop - snapshot.textarea.clientHeight / 2);
        snapshot.textarea.dispatchEvent(new Event("scroll", { bubbles: true }));
      }
      return;
    }
    const first = snapshot.parts.find((part) => part.end > match.start);
    const last = snapshot.parts.find((part) => part.end >= match.end);
    if (!first || !last) return;
    const range = document.createRange();
    range.setStart(first.node, match.start - first.start);
    range.setEnd(last.node, match.end - last.start);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
    first.node.parentElement?.scrollIntoView({ block: "center" });
  }, [current, index, matches, snapshot]);

  useEffect(() => {
    const navigate = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing || event.altKey || !(event.target instanceof Node) || !target.current?.contains(event.target)) return;
      const next = event.key === "F3" && !event.metaKey && !event.ctrlKey ||
        event.key.toLowerCase() === "g" && event.metaKey !== event.ctrlKey;
      if (!next) return;
      event.preventDefault();
      event.stopPropagation();
      move(event.shiftKey ? -1 : 1);
    };
    document.addEventListener("keydown", navigate);
    return () => document.removeEventListener("keydown", navigate);
  }, [move, target]);

  return <EscapeScope onEscape={onClose} exclusive={false}><div role="search" aria-label={m.pdf_preview_find()} className="flex items-center gap-1 border-b border-border px-3 py-1">
    <input ref={input} className="min-w-0 flex-1 bg-background text-sm text-text" aria-label={m.pdf_preview_find()} value={query} onChange={(event) => { setQuery(event.target.value); setCurrent(-1); }}
      onKeyDown={(event) => {
        if (event.key === "Enter") { event.preventDefault(); move(event.shiftKey ? -1 : 1); }
      }} />
    <span aria-live="polite" className="text-xs text-muted">{query ? `${fmtNumber(matches.length && current >= 0 ? index + 1 : 0)} / ${fmtNumber(matches.length)}` : ""}</span>
    <Tooltip interactive content={m.pdf_preview_previous_match()}><IconButton size="small" aria-label={m.pdf_preview_previous_match()} disabled={!matches.length} onClick={() => move(-1)}><ChevronUp size={14} /></IconButton></Tooltip>
    <Tooltip interactive content={m.pdf_preview_next_match()}><IconButton size="small" aria-label={m.pdf_preview_next_match()} disabled={!matches.length} onClick={() => move(1)}><ChevronDown size={14} /></IconButton></Tooltip>
    <Tooltip interactive content={m.chat_search_close()}><IconButton size="small" aria-label={m.chat_search_close()} onClick={onClose}><X size={14} /></IconButton></Tooltip>
  </div></EscapeScope>;
}
