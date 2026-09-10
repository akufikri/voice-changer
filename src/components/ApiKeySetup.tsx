import { useState } from "react";
import { api } from "../lib/tauri";

interface Props {
  onConfigured: () => void;
}

export function ApiKeySetup({ onConfigured }: Props) {
  const [apiKey, setApiKey] = useState("");
  const [voiceId, setVoiceId] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleSave() {
    setError(null);
    setLoading(true);
    try {
      await api.setElevenlabsKey(apiKey, voiceId);
      onConfigured();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="vc-setup">
      <div className="vc-setup-header">
        <h2 className="vc-setup-title">Connect ElevenLabs</h2>
        <p className="vc-setup-desc">
          API key stored in macOS Keychain — never logged or transmitted.
        </p>
      </div>

      <div className="vc-setup-fields">
        <div className="vc-field">
          <label className="vc-label">API Key</label>
          <input
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder="sk_…"
            autoComplete="off"
            className="vc-input"
          />
        </div>

        <div className="vc-field">
          <label className="vc-label">Default Voice ID</label>
          <input
            type="text"
            value={voiceId}
            onChange={(e) => setVoiceId(e.target.value)}
            placeholder="voice ID from ElevenLabs"
            autoComplete="off"
            className="vc-input"
          />
        </div>
      </div>

      {error && (
        <div className="vc-notice vc-notice--red">
          <span className="vc-notice-body">{error}</span>
        </div>
      )}

      <button
        onClick={handleSave}
        disabled={!apiKey || !voiceId || loading}
        className="vc-btn-primary"
      >
        {loading ? "Saving…" : "Save & Continue"}
      </button>
    </div>
  );
}
