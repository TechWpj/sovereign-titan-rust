import { useState, useRef, useEffect } from 'react';
import { Send, Square, Layers, Mic, MicOff } from 'lucide-react';

export default function MessageInput({
  onSend,
  onStop,
  isStreaming,
  disabled,
  deepMode,
  onToggleDeep,
  isRecording,
  onStartRecording,
  onStopRecording,
}) {
  const [value, setValue] = useState('');
  const textareaRef = useRef(null);

  useEffect(() => {
    if (!isStreaming) textareaRef.current?.focus();
  }, [isStreaming]);

  function handleSubmit() {
    const trimmed = value.trim();
    if (!trimmed || isStreaming) return;
    onSend(trimmed);
    setValue('');
  }

  function handleKeyDown(e) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  }

  async function handleMicClick() {
    if (isRecording) {
      try {
        const text = await onStopRecording();
        if (text) {
          setValue((prev) => (prev ? prev + ' ' + text : text));
        }
      } catch {
        // ignored — error logged in hook
      }
    } else {
      try {
        await onStartRecording();
      } catch {
        // mic access denied
      }
    }
  }

  return (
    <div className="border-t border-titan-border bg-titan-bg px-4 py-3">
      <div className="mx-auto flex max-w-3xl items-end gap-2">
        {onToggleDeep && (
          <button
            onClick={onToggleDeep}
            className={`flex h-11 shrink-0 items-center gap-1.5 rounded-xl border px-3 text-xs font-medium transition-colors ${
              deepMode
                ? 'border-amber-500/50 bg-amber-500/10 text-amber-400 hover:bg-amber-500/20'
                : 'border-titan-border bg-titan-surface text-titan-text-muted hover:text-titan-text hover:border-titan-accent'
            }`}
            title={deepMode ? 'Deep Analysis mode ON — click to switch to normal' : 'Switch to Deep Analysis mode'}
          >
            <Layers size={14} />
            <span>{deepMode ? 'Deep' : 'Normal'}</span>
          </button>
        )}

        {onStartRecording && (
          <button
            onClick={handleMicClick}
            className={`flex h-11 w-11 shrink-0 items-center justify-center rounded-xl border transition-colors ${
              isRecording
                ? 'border-red-500/50 bg-red-500/10 text-red-400 animate-pulse hover:bg-red-500/20'
                : 'border-titan-border bg-titan-surface text-titan-text-muted hover:text-titan-text hover:border-titan-accent'
            }`}
            title={isRecording ? 'Stop recording — click to transcribe' : 'Record from microphone'}
          >
            {isRecording ? <MicOff size={16} /> : <Mic size={16} />}
          </button>
        )}

        <textarea
          ref={textareaRef}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={isRecording ? 'Recording... click mic to stop' : 'Send a message...'}
          disabled={disabled}
          rows={1}
          className="max-h-40 min-h-[44px] flex-1 resize-none rounded-xl border border-titan-border bg-titan-surface px-4 py-3 text-sm text-titan-text placeholder-titan-text-muted outline-none focus:border-titan-accent"
        />

        {isStreaming ? (
          <button
            onClick={onStop}
            className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-red-600 text-white transition-colors hover:bg-red-700"
            title="Stop generating"
          >
            <Square size={16} />
          </button>
        ) : (
          <button
            onClick={handleSubmit}
            disabled={!value.trim() || disabled}
            className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-titan-accent text-white transition-colors hover:bg-titan-accent-hover disabled:opacity-40 disabled:hover:bg-titan-accent"
            title="Send message"
          >
            <Send size={16} />
          </button>
        )}
      </div>
    </div>
  );
}
