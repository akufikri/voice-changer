import { useEffect, useState } from "react";
import { api, EngineStatus } from "../lib/tauri";

export function useEngineStatus(pollMs = 1000) {
  const [status, setStatus] = useState<EngineStatus>({
    state: "Idle",
    virtual_mic_name: null,
    monitor_active: false,
  });

  useEffect(() => {
    const id = setInterval(async () => {
      try {
        const s = await api.getEngineStatus();
        setStatus(s);
      } catch {
        // engine not started yet
      }
    }, pollMs);
    return () => clearInterval(id);
  }, [pollMs]);

  return status;
}
