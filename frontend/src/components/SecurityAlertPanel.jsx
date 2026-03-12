import { useState, useEffect, useMemo } from 'react';
import { Shield, AlertTriangle, XCircle, Info, RefreshCw, Scan, ChevronDown, ChevronUp } from 'lucide-react';
import { getSecurityEvents, triggerSecurityScan } from '../api/client';

const SEVERITY_CONFIG = {
  notable: {
    border: 'border-yellow-500',
    bg: 'bg-yellow-500/5',
    badge: 'bg-yellow-500/20 text-yellow-400',
    icon: <Info size={13} className="text-yellow-400" />,
    label: 'Notable',
  },
  concerning: {
    border: 'border-orange-500',
    bg: 'bg-orange-500/5',
    badge: 'bg-orange-500/20 text-orange-400',
    icon: <AlertTriangle size={13} className="text-orange-400" />,
    label: 'Concerning',
  },
  critical: {
    border: 'border-red-500',
    bg: 'bg-red-500/10',
    badge: 'bg-red-500/20 text-red-400',
    icon: <XCircle size={13} className="text-red-400" />,
    label: 'Critical',
  },
};

const THREAT_COLORS = {
  normal: 'text-green-400',
  elevated: 'text-orange-400',
  critical: 'text-red-400',
};

function formatTimestamp(ts) {
  if (!ts) return '';
  const d = typeof ts === 'number' ? new Date(ts * 1000) : new Date(ts);
  if (isNaN(d.getTime())) return '';
  return d.toLocaleTimeString('en-US', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  });
}

function isRecentEvent(ts, thresholdSeconds) {
  if (!ts) return false;
  const eventTime = typeof ts === 'number' ? ts * 1000 : new Date(ts).getTime();
  return Date.now() - eventTime < thresholdSeconds * 1000;
}

export default function SecurityAlertPanel({ liveAlerts = [] }) {
  const [persistedEvents, setPersistedEvents] = useState([]);
  const [loading, setLoading] = useState(true);
  const [filterLevel, setFilterLevel] = useState('all');
  const [scanning, setScanning] = useState(false);
  const [scanReport, setScanReport] = useState(null);
  const [reportExpanded, setReportExpanded] = useState(true);

  async function fetchEvents() {
    setLoading(true);
    try {
      const data = await getSecurityEvents(null, 100);
      setPersistedEvents(data.events || data || []);
    } catch {
      // Silently handle fetch errors; persisted list remains as-is
    } finally {
      setLoading(false);
    }
  }

  async function handleScanNow() {
    setScanning(true);
    setScanReport(null);
    try {
      const report = await triggerSecurityScan();
      setScanReport(report);
      setReportExpanded(true);
      // Refresh events after scan completes
      await fetchEvents();
    } catch {
      setScanReport({ error: 'Scan failed. Is consciousness running?' });
    } finally {
      setScanning(false);
    }
  }

  useEffect(() => {
    fetchEvents();
    const interval = setInterval(fetchEvents, 60_000);
    return () => clearInterval(interval);
  }, []);

  // Merge live alerts with persisted events, dedup by id, sort descending by timestamp
  const mergedEvents = useMemo(() => {
    const byId = new Map();
    for (const evt of persistedEvents) {
      if (evt.id) byId.set(evt.id, evt);
    }
    for (const evt of liveAlerts) {
      if (evt.id) byId.set(evt.id, evt);
    }
    return Array.from(byId.values()).sort((a, b) => {
      const tsA = typeof a.timestamp === 'number' ? a.timestamp : new Date(a.timestamp).getTime() / 1000;
      const tsB = typeof b.timestamp === 'number' ? b.timestamp : new Date(b.timestamp).getTime() / 1000;
      return tsB - tsA;
    });
  }, [persistedEvents, liveAlerts]);

  // Apply filter
  const filteredEvents = useMemo(() => {
    if (filterLevel === 'all') return mergedEvents;
    return mergedEvents.filter((evt) => evt.level === filterLevel);
  }, [mergedEvents, filterLevel]);

  // Badge count: critical events in last 10 minutes
  const criticalRecentCount = useMemo(() => {
    return mergedEvents.filter(
      (evt) => evt.level === 'critical' && isRecentEvent(evt.timestamp, 600)
    ).length;
  }, [mergedEvents]);

  const filterButtons = [
    { key: 'all', label: 'All' },
    { key: 'notable', label: 'Notable' },
    { key: 'concerning', label: 'Concerning' },
    { key: 'critical', label: 'Critical' },
  ];

  return (
    <div className="flex flex-col h-full bg-titan-surface text-titan-text">
      {/* Header */}
      <div className="flex items-center gap-2 px-4 py-3 border-b border-titan-border">
        <Shield size={18} className="text-red-400" />
        <h2 className="text-sm font-semibold">Security Events</h2>
        {criticalRecentCount > 0 && (
          <span className="rounded-full bg-red-500/20 px-2 py-0.5 text-[10px] font-medium text-red-400">
            {criticalRecentCount} critical
          </span>
        )}
        <div className="ml-auto flex items-center gap-1">
          <button
            onClick={handleScanNow}
            disabled={scanning}
            className="rounded-md px-2 py-1 text-[11px] font-medium bg-titan-accent/15 text-titan-accent hover:bg-titan-accent/25 transition-colors disabled:opacity-40 flex items-center gap-1"
            title="Run manual security scan"
          >
            <Scan size={12} className={scanning ? 'animate-spin' : ''} />
            {scanning ? 'Scanning...' : 'Scan Now'}
          </button>
          <button
            onClick={fetchEvents}
            disabled={loading}
            className="rounded-md p-1.5 text-titan-text-muted hover:text-titan-text hover:bg-titan-surface/80 transition-colors disabled:opacity-40"
            title="Refresh events"
          >
            <RefreshCw size={14} className={loading ? 'animate-spin' : ''} />
          </button>
        </div>
      </div>

      {/* Scan Report (collapsible) */}
      {scanReport && (
        <div className="border-b border-titan-border">
          <button
            onClick={() => setReportExpanded(!reportExpanded)}
            className="flex items-center gap-2 w-full px-4 py-2 text-left hover:bg-titan-surface/80 transition-colors"
          >
            <Shield size={14} className="text-titan-accent" />
            <span className="text-[11px] font-semibold text-titan-accent">Scan Report</span>
            {scanReport.anomalies && (
              <span className={`text-[10px] font-mono ${scanReport.anomalies.length > 0 ? 'text-orange-400' : 'text-green-400'}`}>
                {scanReport.anomalies.length} anomalies
              </span>
            )}
            <span className="ml-auto">
              {reportExpanded ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
            </span>
          </button>
          {reportExpanded && (
            <div className="px-4 pb-3 space-y-2">
              {scanReport.error ? (
                <p className="text-[11px] text-red-400">{scanReport.error}</p>
              ) : (
                <>
                  <div className="flex items-center gap-3 text-[11px]">
                    <span className="text-titan-text-muted">
                      Connections: <span className="font-mono text-titan-text">{scanReport.connections ?? 0}</span>
                    </span>
                    <span className="text-titan-text-muted">
                      Baseline: <span className="font-mono text-titan-text">{scanReport.baseline_size ?? 0}</span> processes
                    </span>
                    <span className="text-titan-text-muted">
                      Scanned: <span className="font-mono text-titan-text">{formatTimestamp(scanReport.timestamp)}</span>
                    </span>
                  </div>

                  {scanReport.anomalies && scanReport.anomalies.length > 0 && (
                    <div className="space-y-1">
                      {scanReport.anomalies.map((a, i) => (
                        <div key={i} className="rounded-md bg-orange-500/5 border-l-2 border-orange-500 px-2 py-1 text-[11px]">
                          <span className="font-mono text-orange-400">{a.process}</span>
                          <span className="text-titan-text-muted mx-1">{a.remote}:{a.port}</span>
                          <span className="text-titan-text-muted/70">score: {a.score}</span>
                          {a.reason && <span className="text-titan-text-muted/60 ml-1">({a.reason})</span>}
                        </div>
                      ))}
                    </div>
                  )}

                  {scanReport.summary && (
                    <p className="text-[11px] leading-relaxed text-titan-text-muted/80 italic">
                      {scanReport.summary}
                    </p>
                  )}
                </>
              )}
            </div>
          )}
        </div>
      )}

      {/* Filter buttons */}
      <div className="flex gap-1 px-4 py-2 border-b border-titan-border">
        {filterButtons.map((btn) => (
          <button
            key={btn.key}
            onClick={() => setFilterLevel(btn.key)}
            className={`rounded-md px-2.5 py-1 text-[11px] font-medium transition-colors ${
              filterLevel === btn.key
                ? 'bg-titan-accent/20 text-titan-accent'
                : 'text-titan-text-muted hover:text-titan-text hover:bg-titan-surface/80'
            }`}
          >
            {btn.label}
          </button>
        ))}
      </div>

      {/* Event list */}
      <div className="flex-1 overflow-y-auto px-4 py-2 space-y-2">
        {filteredEvents.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-16 text-titan-text-muted">
            <Shield size={40} className="mb-3 opacity-30" />
            <p className="text-sm">No security events recorded</p>
          </div>
        ) : (
          filteredEvents.map((evt) => {
            const config = SEVERITY_CONFIG[evt.level] || SEVERITY_CONFIG.notable;
            const shouldPulse =
              evt.level === 'critical' && isRecentEvent(evt.timestamp, 60);

            return (
              <div
                key={evt.id}
                className={`rounded-md border-l-4 ${config.border} ${config.bg} ${
                  shouldPulse ? 'animate-pulse' : ''
                } px-3 py-2`}
              >
                {/* Top row: badge + timestamp */}
                <div className="flex items-center gap-2 mb-1">
                  <span
                    className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium ${config.badge}`}
                  >
                    {config.icon}
                    {config.label}
                  </span>
                  <span className="text-[10px] text-titan-text-muted font-mono ml-auto">
                    {formatTimestamp(evt.timestamp)}
                  </span>
                </div>

                {/* Process + remote info */}
                {(evt.process_name || evt.remote_ip) && (
                  <div className="flex items-center gap-2 text-[11px] text-titan-text-muted mb-0.5">
                    {evt.process_name && (
                      <span className="font-mono">{evt.process_name}</span>
                    )}
                    {evt.remote_ip && (
                      <span className="font-mono text-titan-text-muted/70">
                        {evt.remote_ip}
                        {evt.remote_port ? `:${evt.remote_port}` : ''}
                      </span>
                    )}
                  </div>
                )}

                {/* Action taken */}
                {evt.action && (
                  <div className="text-[11px] text-titan-text-muted/80 mb-0.5">
                    <span className="font-medium text-titan-text-muted">Action:</span>{' '}
                    <span className="font-mono">{evt.action}</span>
                  </div>
                )}

                {/* Details */}
                {evt.details && (
                  <p className="text-[11px] leading-relaxed text-titan-text-muted/70">
                    {evt.details}
                  </p>
                )}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
