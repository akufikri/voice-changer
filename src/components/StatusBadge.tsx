interface Props {
  state: string;
}

const stateClass: Record<string, string> = {
  Idle:         "vc-badge--idle",
  Initializing: "vc-badge--initializing",
  Ready:        "vc-badge--ready",
  Processing:   "vc-badge--processing",
  Degraded:     "vc-badge--degraded",
};

export function StatusBadge({ state }: Props) {
  const cls = stateClass[state] ?? "vc-badge--degraded";
  return (
    <span className={`vc-badge ${cls}`}>
      <span className="vc-badge-dot" />
      {state}
    </span>
  );
}
