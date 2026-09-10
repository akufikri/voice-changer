import { AudioDevice } from "../lib/tauri";

interface Props {
  devices: AudioDevice[];
  selected: string | null;
  onSelect: (id: string) => void;
  disabled?: boolean;
}

export function DeviceSelector({ devices, selected, onSelect, disabled }: Props) {
  return (
    <div className="vc-field">
      <label className="vc-label">Microphone</label>
      <select
        value={selected ?? ""}
        onChange={(e) => onSelect(e.target.value)}
        disabled={disabled}
        className="vc-select"
      >
        <option value="" disabled>Select device…</option>
        {devices.map((d) => (
          <option key={d.id} value={d.id}>
            {d.name}{d.is_default ? " ·  default" : ""}
          </option>
        ))}
      </select>
    </div>
  );
}
