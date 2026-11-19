/** Find matching visible experiments and keep the context on paths to their roots. */
export function graphMatches(
  nodes: ReadonlyArray<{ id: string; title: string; slug: string; status: string | null }>,
  edges: ReadonlyArray<{ source: string; target: string }>,
  query: string,
  status: string,
) {
  const term = query.trim().toLocaleLowerCase();
  const matching = new Set(nodes.filter((node) =>
    (!term || `${node.title}\n${node.slug}`.toLocaleLowerCase().includes(term)) &&
    (status === "all" || node.status === status || (status === "flagged" && node.status === "flagged_ended")),
  ).map((node) => node.id));
  const parent = new Map(edges.map((edge) => [edge.target, edge.source]));
  const visible = new Set(matching);
  for (const id of matching) {
    let current = parent.get(id);
    while (current && !visible.has(current)) {
      visible.add(current);
      current = parent.get(current);
    }
  }
  return { matching, visible };
}

/** A completed programmatic move belongs only to the canvas that started it. */
export async function saveGraphMove<T, V>(
  flow: T,
  move: (flow: T) => Promise<boolean>,
  current: () => boolean,
  viewport: (flow: T) => V,
  save: (viewport: V) => void,
) {
  if (await move(flow) && current()) save(viewport(flow));
}
