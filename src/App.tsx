import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import "./App.css";
import { ApiKeySetup } from "./components/ApiKeySetup";
import { DeviceSelector } from "./components/DeviceSelector";
import { StatusBadge } from "./components/StatusBadge";
import { VoiceSelector } from "./components/VoiceSelector";
import { useEngineStatus } from "./hooks/useEngineState";
import { api, AudioDevice, Voice } from "./lib/tauri";

export default function App() {
  const [hasKey, setHasKey] = useState<boolean | null>(null);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [voices, setVoices] = useState<Voice[]>([]);
  const [selectedDevice, setSelectedDevice] = useState<string | null>(null);
  const [selectedVoice, setSelectedVoice] = useState<string | null>(null);
  const [voicesLoading, setVoicesLoading] = useState(false);
  const [running, setRunning] = useState(false);
  const [processing, setProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const { state: engineState, virtual_mic_name, monitor_active } = useEngineStatus();
  const [deviceNotice, setDeviceNotice] = useState<string | null>(null);
  const noticeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    api.getSavedApiKey().then(setHasKey).catch(() => setHasKey(false));
  }, []);

  useEffect(() => {
    if (!hasKey) return;
    const unlisten = listen<{ added: string[]; removed: string[] }>(
      "devices-changed",
      ({ payload }) => {
        api.getAudioDevices().then((devs) => {
          setDevices(devs);
          if (payload.added.length > 0 && !running) {
            const newDev = devs.find((d) => payload.added.includes(d.id));
            if (newDev) {
              setSelectedDevice(newDev.id);
              setDeviceNotice(`"${newDev.name}" connected`);
              if (noticeTimer.current) clearTimeout(noticeTimer.current);
              noticeTimer.current = setTimeout(() => setDeviceNotice(null), 4000);
            }
          }
          if (payload.removed.length > 0) {
            setDeviceNotice(`Disconnected: ${payload.removed.join(", ")}`);
            if (noticeTimer.current) clearTimeout(noticeTimer.current);
            noticeTimer.current = setTimeout(() => setDeviceNotice(null), 5000);
          }
        });
      }
    );
    return () => { unlisten.then((fn) => fn()); };
  }, [hasKey, running]);

  useEffect(() => {
    if (!hasKey) return;
    api.getAudioDevices()
      .then(setDevices)
      .catch((e) => setError(String(e)));

    setVoicesLoading(true);
    api.getVoices()
      .then(setVoices)
      .catch((e) => setError(String(e)))
      .finally(() => setVoicesLoading(false));

    api.loadSettings().then((s) => {
      if (s.selected_device_id) setSelectedDevice(s.selected_device_id);
      if (s.selected_voice_id) setSelectedVoice(s.selected_voice_id);
    }).catch(() => {});
  }, [hasKey]);

  async function handleStart() {
    setError(null);
    try {
      await api.startVoiceChanger(selectedDevice ?? undefined, selectedVoice ?? undefined);
      setRunning(true);
      await api.saveSettings({
        selected_device_id: selectedDevice,
        selected_voice_id: selectedVoice,
      });
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleStop() {
    await api.stopVoiceChanger().catch(() => {});
    setRunning(false);
    setProcessing(false);
  }

  async function handleToggleProcessing() {
    const next = !processing;
    try {
      await api.setProcessingEnabled(next);
      setProcessing(next);
    } catch (e) {
      setError(String(e));
    }
  }

  if (hasKey === null) {
    return (
      <div className="vc-center-screen">
        <div className="vc-loading-dot" />
      </div>
    );
  }

  if (!hasKey) {
    return (
      <div className="vc-center-screen">
        <ApiKeySetup onConfigured={() => setHasKey(true)} />
      </div>
    );
  }

  return (
    <div className="vc-app">
      <header className="vc-header">
        <div>
          <h1 className="vc-wordmark">Voice Changer</h1>
          <p className="vc-subtext">ElevenLabs · Google Meet</p>
        </div>
        <StatusBadge state={engineState} />
      </header>

      <div className="vc-card">
        <DeviceSelector
          devices={devices}
          selected={selectedDevice}
          onSelect={setSelectedDevice}
          disabled={running}
        />

        <VoiceSelector
          voices={voices}
          selected={selectedVoice}
          onSelect={setSelectedVoice}
          loading={voicesLoading}
          disabled={running}
        />

        {deviceNotice && (
          <div className="vc-notice vc-notice--amber">
            <span className="vc-notice-body">{deviceNotice}</span>
            <button
              className="vc-notice-dismiss"
              onClick={() => setDeviceNotice(null)}
              aria-label="Dismiss"
            >
              ✕
            </button>
          </div>
        )}

        {error && (
          <div className="vc-notice vc-notice--red">
            <span className="vc-notice-body">{error}</span>
            <button
              className="vc-notice-dismiss"
              onClick={() => setError(null)}
              aria-label="Dismiss"
            >
              ✕
            </button>
          </div>
        )}

        <div className="vc-actions">
          {!running ? (
            <button
              onClick={handleStart}
              disabled={!selectedDevice || !selectedVoice}
              className="vc-btn-primary"
            >
              Start Engine
            </button>
          ) : (
            <>
              <button
                onClick={handleToggleProcessing}
                className={`vc-btn-toggle${processing ? " is-active" : ""}`}
                title={processing ? "AI voice active — click to bypass" : "Bypass mode — click to enable AI voice"}
              >
                {processing ? "AI Voice" : "Bypass"}
              </button>
              <button
                onClick={() =>
                  api.setMonitorEnabled(!monitor_active).catch((e) =>
                    setError(String(e))
                  )
                }
                className={`vc-btn-icon${monitor_active ? " is-active" : ""}`}
                title={monitor_active ? "Stop monitoring output" : "Hear your converted voice through speakers"}
                aria-label={monitor_active ? "Disable monitor" : "Enable monitor"}
              >
                {monitor_active ? "🔊" : "🔇"}
              </button>
              <button onClick={handleStop} className="vc-btn-stop">
                Stop
              </button>
            </>
          )}
        </div>
      </div>

      {running && (
        <div className="vc-instr">
          <div className="vc-instr-live">
            <span className="vc-instr-dot" />
            <span className="vc-instr-device">
              {virtual_mic_name ?? "BlackHole 2ch"}
            </span>
          </div>
          <p className="vc-instr-heading">In Google Meet:</p>
          <ol className="vc-steps">
            <li>Settings → Audio</li>
            <li>
              Microphone →{" "}
              <strong>{virtual_mic_name ?? "BlackHole 2ch"}</strong>
            </li>
            <li>Enable AI Voice above, then speak</li>
          </ol>
        </div>
      )}

      <button className="vc-link" onClick={() => setHasKey(false)}>
        Change API Key
      </button>
    </div>
  );
}
