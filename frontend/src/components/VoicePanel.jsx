import { useState, useEffect, useCallback, useRef } from 'react';
import { Mic, Square, ChevronDown, ChevronRight } from 'lucide-react';
import { fetchVoiceStatus, toggleVoiceListen, toggleVoiceAutoRespond, updateVoiceSettings, stopVoiceSpeaking } from '../api/client';

function Toggle({ enabled, onClick }) {
  return (
    <button
      onClick={onClick}
      className={`relative h-5 w-9 shrink-0 rounded-full transition-colors ${
        enabled ? 'bg-green-500' : 'bg-titan-border'
      }`}
    >
      <span
        className={`absolute top-0.5 left-0.5 h-4 w-4 rounded-full bg-white transition-transform ${
          enabled ? 'translate-x-4' : ''
        }`}
      />
    </button>
  );
}

export default function VoicePanel() {
  const [collapsed, setCollapsed] = useState(false);
  const [listening, setListening] = useState(false);
  const [autoRespond, setAutoRespond] = useState(false);
  const [wakeWord, setWakeWord] = useState('titan');
  const [phase, setPhase] = useState('idle');
  const [energyThreshold, setEnergyThreshold] = useState(80);
  const commitTimer = useRef(null);

  const refresh = useCallback(async () => {
    const status = await fetchVoiceStatus();
    setListening(!!status.continuous_listening);
    setAutoRespond(!!status.auto_respond);
    if (status.wake_word) setWakeWord(status.wake_word);
    if (status.phase) setPhase(status.phase);
    if (status.energy_threshold != null) setEnergyThreshold(status.energy_threshold);
  }, []);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 1000);
    return () => clearInterval(id);
  }, [refresh]);

  async function handleToggleListen() {
    try {
      const res = await toggleVoiceListen();
      setListening(!!res.listening);
    } catch {
      await refresh();
    }
  }

  async function handleToggleAutoRespond() {
    try {
      const res = await toggleVoiceAutoRespond();
      setAutoRespond(!!res.auto_respond);
    } catch {
      await refresh();
    }
  }

  async function handleStopSpeaking() {
    try {
      await stopVoiceSpeaking();
      await refresh();
    } catch { /* ignore */ }
  }

  function handleEnergyChange(e) {
    const val = Number(e.target.value);
    setEnergyThreshold(val);
    if (commitTimer.current) clearTimeout(commitTimer.current);
    commitTimer.current = setTimeout(async () => {
      try {
        await updateVoiceSettings({ energy_threshold: val });
      } catch { /* ignore */ }
    }, 500);
  }

  const isActive = phase !== 'idle' && phase !== 'scanning';

  return (
    <div className="border-t border-titan-border">
      <button
        onClick={() => setCollapsed(!collapsed)}
        className="flex w-full items-center gap-2 px-3 py-2 text-xs font-medium text-titan-text-muted hover:text-titan-text transition-colors"
      >
        <Mic size={13} className={isActive ? 'text-green-400 animate-pulse' : 'text-cyan-400'} />
        <span>Voice Control</span>
        <span className="ml-auto">
          {collapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
        </span>
      </button>

      {!collapsed && (
        <div className="px-3 pb-2 space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-[11px] text-titan-text-muted">Always-On Listening</span>
            <Toggle enabled={listening} onClick={handleToggleListen} />
          </div>
          <div className="flex items-center justify-between">
            <span className="text-[11px] text-titan-text-muted">Auto Voice Respond</span>
            <Toggle enabled={autoRespond} onClick={handleToggleAutoRespond} />
          </div>
          <div className="flex items-center justify-between">
            <span className="text-[11px] text-titan-text-muted">Status</span>
            <div className="flex items-center gap-1.5">
              <span className={`text-[11px] font-medium ${
                phase === 'idle' || phase === 'scanning' ? 'text-titan-text-muted/50'
                  : phase === 'listening' ? 'text-cyan-400'
                  : phase === 'recording' ? 'text-green-400'
                  : phase === 'processing' ? 'text-yellow-400'
                  : phase === 'speaking' ? 'text-purple-400'
                  : 'text-titan-text-muted/50'
              }`}>
                {phase === 'idle' ? 'Idle'
                  : phase === 'scanning' ? 'Waiting for wake word'
                  : phase === 'listening' ? 'Listening...'
                  : phase === 'recording' ? 'Recording...'
                  : phase === 'processing' ? 'Processing...'
                  : phase === 'speaking' ? 'Speaking...'
                  : 'Idle'}
              </span>
              {phase === 'speaking' && (
                <button
                  onClick={handleStopSpeaking}
                  className="flex items-center gap-0.5 px-1.5 py-0.5 rounded text-[10px] font-medium
                    bg-red-500/20 text-red-400 hover:bg-red-500/30 transition-colors"
                  title="Stop speaking"
                >
                  <Square size={8} fill="currentColor" />
                  Stop
                </button>
              )}
            </div>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-[11px] text-titan-text-muted">Wake word</span>
            <span className="text-[11px] text-titan-text-muted/70">&ldquo;{wakeWord}&rdquo;</span>
          </div>
          <div className="space-y-1">
            <div className="flex items-center justify-between">
              <span className="text-[11px] text-titan-text-muted">Energy Threshold</span>
              <span className="text-[11px] text-titan-text-muted/70 tabular-nums">{Math.round(energyThreshold)}</span>
            </div>
            <input
              type="range"
              min="10"
              max="500"
              step="5"
              value={energyThreshold}
              onChange={handleEnergyChange}
              className="w-full h-1 bg-titan-border rounded-full appearance-none cursor-pointer
                [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-3 [&::-webkit-slider-thumb]:h-3
                [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-cyan-400
                [&::-moz-range-thumb]:w-3 [&::-moz-range-thumb]:h-3
                [&::-moz-range-thumb]:rounded-full [&::-moz-range-thumb]:bg-cyan-400 [&::-moz-range-thumb]:border-0"
            />
            <div className="flex justify-between text-[9px] text-titan-text-muted/40">
              <span>Sensitive</span>
              <span>Aggressive</span>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
