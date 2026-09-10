import { Voice } from "../lib/tauri";

interface Props {
  voices: Voice[];
  selected: string | null;
  onSelect: (id: string) => void;
  loading?: boolean;
  disabled?: boolean;
}

export function VoiceSelector({ voices, selected, onSelect, loading, disabled }: Props) {
  return (
    <div className="vc-field">
      <label className="vc-label">Voice</label>
      <select
        value={selected ?? ""}
        onChange={(e) => onSelect(e.target.value)}
        disabled={disabled || loading}
        className="vc-select"
      >
        <option value="" disabled>
          {loading ? "Loading voices…" : "Select voice…"}
        </option>
        {voices.map((v) => (
          <option key={v.id} value={v.id}>
            {v.name}
          </option>
        ))}
      </select>
    </div>
  );
}
