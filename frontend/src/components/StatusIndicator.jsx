import { useState, useEffect } from 'react';
import { checkHealth } from '../api/client';
import { HEALTH_POLL_INTERVAL } from '../lib/constants';

export default function StatusIndicator({ timeContext }) {
  const [healthy, setHealthy] = useState(null);

  useEffect(() => {
    let mounted = true;

    async function poll() {
      const ok = await checkHealth();
      if (mounted) setHealthy(ok);
    }

    poll();
    const id = setInterval(poll, HEALTH_POLL_INTERVAL);
    return () => {
      mounted = false;
      clearInterval(id);
    };
  }, []);

  const color =
    healthy === null
      ? 'bg-yellow-500'
      : healthy
        ? 'bg-emerald-500'
        : 'bg-red-500';

  const label =
    healthy === null ? 'Checking...' : healthy ? 'Online' : 'Offline';

  // Build extra info from timeContext
  const extra = [];
  if (timeContext) {
    if (timeContext.session_uptime) extra.push(timeContext.session_uptime);
    if (timeContext.time_of_day) {
      extra.push(timeContext.time_of_day.charAt(0).toUpperCase() + timeContext.time_of_day.slice(1));
    }
  }

  return (
    <div className="flex items-center gap-1.5 text-xs text-titan-text-muted">
      <span className={`inline-block h-2 w-2 rounded-full ${color}`} />
      {label}
      {extra.length > 0 && (
        <span className="opacity-60">
          {' · '}
          {extra.join(' · ')}
        </span>
      )}
    </div>
  );
}
