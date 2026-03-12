import { useState, useEffect, useRef, useCallback } from 'react';
import { Play, Square, Check, AlertTriangle, Loader, ChevronDown, ChevronRight, Image, Eye, EyeOff, KeyRound } from 'lucide-react';
import { startAutomation, confirmAutomation, cancelAutomation, listAutomations } from '../api/client';
import { getApiKey } from '../lib/storage';

const STATUS_ICONS = {
  navigating: <Loader size={14} className="animate-spin text-titan-accent" />,
  navigated: <Check size={14} className="text-green-400" />,
  discovering: <Loader size={14} className="animate-spin text-titan-accent" />,
  fields_discovered: <Check size={14} className="text-green-400" />,
  mapping: <Loader size={14} className="animate-spin text-titan-accent" />,
  fields_mapped: <Check size={14} className="text-green-400" />,
  confirm_required: <AlertTriangle size={14} className="text-yellow-400" />,
  filling: <Loader size={14} className="animate-spin text-titan-accent" />,
  field_filled: <Check size={14} className="text-green-400" />,
  field_fill_error: <AlertTriangle size={14} className="text-red-400" />,
  submitting: <Loader size={14} className="animate-spin text-titan-accent" />,
  submitted: <Check size={14} className="text-green-400" />,
  verifying: <Loader size={14} className="animate-spin text-titan-accent" />,
  completed: <Check size={14} className="text-green-400" />,
  failed: <AlertTriangle size={14} className="text-red-400" />,
  cancelled: <Square size={14} className="text-titan-text-muted" />,
  screenshot: <Image size={14} className="text-titan-text-muted" />,
  log: <Check size={14} className="text-titan-text-muted" />,
  credentials: <KeyRound size={14} className="text-yellow-400" />,
};

function LogEntry({ entry }) {
  const [expanded, setExpanded] = useState(false);
  const icon = STATUS_ICONS[entry.type] || <Check size={14} className="text-titan-text-muted" />;
  const time = entry.timestamp ? new Date(entry.timestamp).toLocaleTimeString() : '';

  return (
    <div className="flex items-start gap-2 py-1.5 px-2 text-sm border-b border-titan-border/50 last:border-0">
      <span className="mt-0.5 shrink-0">{icon}</span>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-titan-text">{entry.message}</span>
          <span className="text-xs text-titan-text-muted ml-auto shrink-0">{time}</span>
        </div>
        {entry.screenshot_path && (
          <button
            onClick={() => setExpanded(!expanded)}
            className="mt-1 flex items-center gap-1 text-xs text-titan-accent hover:text-titan-accent-hover"
          >
            {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
            Screenshot
          </button>
        )}
        {expanded && entry.screenshot_path && (
          <img
            src={`/screenshots/${entry.screenshot_path.split('/').pop()}`}
            alt="Screenshot"
            className="mt-1 max-w-full rounded border border-titan-border"
          />
        )}
        {entry.type === 'credentials' && entry.credentials && (
          <div className="mt-2 rounded border border-yellow-500/30 bg-yellow-500/10 p-3 text-xs font-mono space-y-1">
            {entry.credentials.email && (
              <div><span className="text-titan-text-muted">Email:</span> <span className="text-titan-text">{entry.credentials.email}</span></div>
            )}
            {entry.credentials.username && (
              <div><span className="text-titan-text-muted">Username:</span> <span className="text-titan-text">{entry.credentials.username}</span></div>
            )}
            {entry.credentials.password && (
              <div><span className="text-titan-text-muted">Password:</span> <span className="text-titan-text">{entry.credentials.password}</span></div>
            )}
            {entry.site && (
              <div><span className="text-titan-text-muted">Site:</span> <span className="text-titan-text">{entry.site}</span></div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function FieldReview({ fields, onConfirm, onCancel }) {
  const [overrides, setOverrides] = useState({});

  const handleChange = (selector, value) => {
    setOverrides(prev => ({ ...prev, [selector]: value }));
  };

  const handleConfirm = () => {
    const hasOverrides = Object.keys(overrides).length > 0;
    onConfirm(hasOverrides ? overrides : null);
  };

  return (
    <div className="mx-2 my-3 rounded-lg border border-titan-border bg-titan-surface p-4">
      <h3 className="text-sm font-semibold text-titan-text mb-3">Field Review</h3>
      <div className="space-y-2">
        {fields.map((field, i) => {
          const label = field.label || field.name || field.selector || `Field ${i + 1}`;
          const isPassword = field.type === 'password' || (field.profile_key || '').includes('password');
          const displayValue = overrides[field.selector] !== undefined
            ? overrides[field.selector]
            : (field.value || '');
          const mapped = !!field.value;
          const source = field.profile_key === 'llm_inferred' ? 'inferred'
                       : mapped ? 'auto' : '';

          return (
            <div key={field.selector || i} className="flex items-center gap-3">
              <span className="text-sm text-titan-text-muted w-28 shrink-0 truncate" title={label}>
                {label}:
              </span>
              <input
                type={isPassword ? 'password' : 'text'}
                value={displayValue}
                onChange={(e) => handleChange(field.selector, e.target.value)}
                placeholder={mapped ? '' : 'Enter value...'}
                className="flex-1 rounded border border-titan-border bg-titan-bg px-2 py-1 text-sm text-titan-text placeholder-titan-text-muted outline-none focus:border-titan-accent"
              />
              <span className={`text-xs shrink-0 w-16 text-right ${
                mapped ? 'text-green-400' : 'text-yellow-400'
              }`}>
                {source ? `✓ ${source}` : '⚠ empty'}
              </span>
            </div>
          );
        })}
      </div>
      <div className="flex justify-end gap-2 mt-4">
        <button
          onClick={onCancel}
          className="px-4 py-1.5 rounded text-sm border border-titan-border text-titan-text-muted hover:bg-titan-surface hover:text-titan-text transition-colors"
        >
          Cancel
        </button>
        <button
          onClick={handleConfirm}
          className="px-4 py-1.5 rounded text-sm bg-titan-accent text-white hover:bg-titan-accent-hover transition-colors"
        >
          Confirm & Fill
        </button>
      </div>
    </div>
  );
}

const STATUS_BADGES = {
  completed: { label: 'Completed', className: 'bg-green-500/20 text-green-400 border-green-500/30' },
  failed: { label: 'Failed', className: 'bg-red-500/20 text-red-400 border-red-500/30' },
  cancelled: { label: 'Cancelled', className: 'bg-gray-500/20 text-gray-400 border-gray-500/30' },
  running: { label: 'Running', className: 'bg-blue-500/20 text-blue-400 border-blue-500/30' },
  pending: { label: 'Pending', className: 'bg-yellow-500/20 text-yellow-400 border-yellow-500/30' },
};

function StatusBadge({ status }) {
  const badge = STATUS_BADGES[status] || STATUS_BADGES.pending;
  return (
    <span className={`inline-flex items-center rounded-full border px-2 py-0.5 text-xs font-medium ${badge.className}`}>
      {badge.label}
    </span>
  );
}

export default function AutomationView() {
  const [input, setInput] = useState('');
  const [headless, setHeadless] = useState(false);
  const [activeTask, setActiveTask] = useState(null);
  const [logEntries, setLogEntries] = useState([]);
  const [confirmFields, setConfirmFields] = useState(null);
  const [history, setHistory] = useState([]);
  const [historyExpanded, setHistoryExpanded] = useState(false);
  const logRef = useRef(null);
  const eventSourceRef = useRef(null);

  // Fetch history from backend
  const fetchHistory = useCallback(async () => {
    try {
      const data = await listAutomations();
      const tasks = data.tasks || data || [];
      setHistory(tasks);
    } catch (err) {
      console.warn('Failed to fetch automation history:', err);
    }
  }, []);

  // Fetch history on mount
  useEffect(() => {
    fetchHistory();
  }, [fetchHistory]);

  // Auto-scroll log
  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [logEntries]);

  // Clean up SSE on unmount
  useEffect(() => {
    return () => {
      if (eventSourceRef.current) {
        eventSourceRef.current.close();
      }
    };
  }, []);

  const connectSSE = useCallback((taskId) => {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
    }

    const key = getApiKey();
    const url = `/v1/automation/${taskId}/stream${key ? `?key=${encodeURIComponent(key)}` : ''}`;
    const es = new EventSource(url);
    eventSourceRef.current = es;

    es.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        setLogEntries(prev => [...prev, data]);

        if (data.type === 'confirm_required') {
          setConfirmFields(data.fields || []);
        }
        if (data.type === 'completed' || data.type === 'failed' || data.type === 'cancelled') {
          setActiveTask(prev => prev ? { ...prev, state: data.type } : prev);
          es.close();
          fetchHistory();
        }
      } catch { /* skip malformed */ }
    };

    es.onerror = () => {
      es.close();
    };
  }, [fetchHistory]);

  const handleStart = async () => {
    if (!input.trim()) return;
    setLogEntries([]);
    setConfirmFields(null);

    try {
      const task = await startAutomation(input.trim(), { headless });
      setActiveTask(task);
      setInput('');
      connectSSE(task.task_id);
    } catch (err) {
      setLogEntries([{
        type: 'failed',
        message: `Failed to start: ${err.message}`,
        timestamp: new Date().toISOString(),
      }]);
    }
  };

  const handleConfirm = async (overrides) => {
    if (!activeTask) return;
    setConfirmFields(null);
    try {
      await confirmAutomation(activeTask.task_id, overrides);
    } catch (err) {
      setLogEntries(prev => [...prev, {
        type: 'failed',
        message: `Confirm failed: ${err.message}`,
        timestamp: new Date().toISOString(),
      }]);
    }
  };

  const handleCancel = async () => {
    if (!activeTask) return;
    setConfirmFields(null);
    try {
      await cancelAutomation(activeTask.task_id);
    } catch (err) {
      setLogEntries(prev => [...prev, {
        type: 'failed',
        message: `Cancel failed: ${err.message}`,
        timestamp: new Date().toISOString(),
      }]);
    }
  };

  const handleKeyDown = (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleStart();
    }
  };

  const isRunning = activeTask && !['completed', 'failed', 'cancelled'].includes(activeTask.state);

  return (
    <div className="flex flex-1 flex-col h-full bg-titan-bg">
      {/* Input bar */}
      <div className="border-b border-titan-border px-4 py-3">
        <div className="flex items-center gap-2 max-w-3xl mx-auto">
          <input
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder='Describe automation (e.g. "Sign up on github.com")'
            disabled={isRunning}
            className="flex-1 rounded-lg border border-titan-border bg-titan-surface px-4 py-2.5 text-sm text-titan-text placeholder-titan-text-muted outline-none focus:border-titan-accent disabled:opacity-50"
          />
          <button
            onClick={() => setHeadless(!headless)}
            disabled={isRunning}
            title={headless ? 'Headless mode (invisible browser)' : 'Headed mode (visible browser window)'}
            className={`shrink-0 rounded-lg border px-3 py-2.5 text-sm transition-colors disabled:opacity-50 ${
              headless
                ? 'border-titan-border text-titan-text-muted hover:text-titan-text hover:bg-titan-surface'
                : 'border-titan-accent text-titan-accent hover:bg-titan-accent/10'
            }`}
          >
            {headless ? <EyeOff size={16} /> : <Eye size={16} />}
          </button>
          {isRunning ? (
            <button
              onClick={handleCancel}
              className="shrink-0 rounded-lg bg-red-600 px-4 py-2.5 text-sm text-white hover:bg-red-500 transition-colors"
            >
              <Square size={16} />
            </button>
          ) : (
            <button
              onClick={handleStart}
              disabled={!input.trim()}
              className="shrink-0 rounded-lg bg-titan-accent px-4 py-2.5 text-sm text-white hover:bg-titan-accent-hover transition-colors disabled:opacity-50"
            >
              <Play size={16} />
            </button>
          )}
        </div>
      </div>

      {/* Log area */}
      <div ref={logRef} className="flex-1 overflow-y-auto px-4 py-3">
        <div className="max-w-3xl mx-auto">
          {logEntries.length === 0 && !activeTask && (
            <div className="text-center text-titan-text-muted py-12">
              <p className="text-lg mb-2">Browser Automation</p>
              <p className="text-sm">
                Describe a task like "Sign up on github.com" or "Log in to twitter.com"
              </p>
              <p className="text-xs mt-1">
                Fields are mapped from your profile. You'll review before anything is submitted.
              </p>
            </div>
          )}

          {logEntries.map((entry, i) => (
            <LogEntry key={i} entry={entry} />
          ))}

          {/* Field review panel */}
          {confirmFields && (
            <FieldReview
              fields={confirmFields}
              onConfirm={handleConfirm}
              onCancel={handleCancel}
            />
          )}

          {/* Completion badge */}
          {activeTask && activeTask.state === 'completed' && (
            <div className="mt-4 rounded-lg border border-green-500/30 bg-green-500/10 p-3 text-sm text-green-400 text-center">
              Automation completed successfully
            </div>
          )}
          {activeTask && activeTask.state === 'failed' && (
            <div className="mt-4 rounded-lg border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-400 text-center">
              Automation failed
            </div>
          )}
        </div>
      </div>

      {/* History table */}
      {history.length > 0 && (
        <div className="border-t border-titan-border px-4 py-2">
          <button
            onClick={() => setHistoryExpanded(!historyExpanded)}
            className="flex items-center gap-1 text-sm font-medium text-titan-text-muted hover:text-titan-text mb-2"
          >
            {historyExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            Automation History ({history.length})
          </button>
          {historyExpanded && (
            <div className="overflow-x-auto max-h-64 overflow-y-auto">
              <table className="w-full text-xs">
                <thead>
                  <tr className="border-b border-titan-border text-left text-titan-text-muted">
                    <th className="py-2 px-2 font-medium">Date</th>
                    <th className="py-2 px-2 font-medium">Description</th>
                    <th className="py-2 px-2 font-medium">Target URL</th>
                    <th className="py-2 px-2 font-medium text-right">Status</th>
                  </tr>
                </thead>
                <tbody>
                  {history.map((h, i) => {
                    const date = h.created_at || h.timestamp
                      ? new Date(h.created_at || h.timestamp).toLocaleString()
                      : '—';
                    const desc = h.description || h.instruction || h.task || '—';
                    const url = h.target_url || h.url || '—';
                    const state = h.state || h.status || 'pending';
                    return (
                      <tr key={h.task_id || i} className="border-b border-titan-border/50 hover:bg-titan-surface/50">
                        <td className="py-1.5 px-2 text-titan-text-muted whitespace-nowrap">{date}</td>
                        <td className="py-1.5 px-2 text-titan-text max-w-xs truncate" title={desc}>{desc}</td>
                        <td className="py-1.5 px-2 text-titan-accent max-w-xs truncate" title={url}>
                          {url !== '—' ? (
                            <a href={url} target="_blank" rel="noopener noreferrer" className="hover:underline">{url}</a>
                          ) : '—'}
                        </td>
                        <td className="py-1.5 px-2 text-right">
                          <StatusBadge status={state} />
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
