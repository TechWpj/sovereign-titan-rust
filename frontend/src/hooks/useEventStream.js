import { useState, useEffect, useCallback } from 'react';
import { listen } from '@tauri-apps/api/event';
import { submitTask as apiSubmitTask, cancelTask as apiCancelTask } from '../api/client';

export function useEventStream() {
  const [thoughts, setThoughts] = useState([]);
  const [tasks, setTasks] = useState([]);
  const [timeContext, setTimeContext] = useState(null);
  const [securityAlerts, setSecurityAlerts] = useState([]);
  const [dialogueMessages, setDialogueMessages] = useState([]);

  useEffect(() => {
    let unlistenSecurity;
    let unlistenSubconscious;

    async function setup() {
      unlistenSecurity = await listen('security-alert', (event) => {
        try {
          const alert = event.payload;
          setSecurityAlerts((prev) => [alert, ...prev.slice(-99)]);
        } catch {}
      });

      unlistenSubconscious = await listen('subconscious-insight', (event) => {
        try {
          const insight = event.payload;
          setThoughts((prev) => [
            {
              category: 'subconscious',
              text: insight.insight,
              timestamp: insight.timestamp * 1000,
            },
            ...prev.slice(-49),
          ]);
        } catch {}
      });
    }

    setup();

    return () => {
      if (unlistenSecurity) unlistenSecurity();
      if (unlistenSubconscious) unlistenSubconscious();
    };
  }, []);

  const submitTask = useCallback(async (description) => {
    const task = await apiSubmitTask(description);
    setTasks((prev) => [{ ...task, progress: [] }, ...prev.filter((t) => t.id !== task.id)]);
    return task;
  }, []);

  const cancelTask = useCallback(async (taskId) => {
    await apiCancelTask(taskId);
    setTasks((prev) =>
      prev.map((t) => (t.id === taskId ? { ...t, status: 'cancelled' } : t))
    );
  }, []);

  return { thoughts, tasks, timeContext, securityAlerts, dialogueMessages, submitTask, cancelTask };
}
