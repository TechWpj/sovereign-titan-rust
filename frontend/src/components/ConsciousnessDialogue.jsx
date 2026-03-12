import { useState, useEffect, useRef, useCallback } from 'react';
import { MessageCircle, Send } from 'lucide-react';

function timeAgo(timestamp) {
  const secs = Math.floor(Date.now() / 1000 - timestamp);
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h ago`;
}

export default function ConsciousnessDialogue({ messages: sseMessages }) {
  const [messages, setMessages] = useState([]);
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const scrollRef = useRef(null);

  // Load history on mount (no-op in Tauri mode — no HTTP backend)
  useEffect(() => {}, []);

  // Merge SSE messages
  useEffect(() => {
    if (sseMessages && sseMessages.length > 0) {
      setMessages((prev) => {
        const latest = sseMessages[sseMessages.length - 1];
        // Dedup by timestamp
        if (prev.some((m) => m.timestamp === latest.timestamp)) return prev;
        return [...prev, latest];
      });
    }
  }, [sseMessages]);

  // Auto-scroll on new messages
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages]);

  const handleSend = useCallback(async () => {
    const text = input.trim();
    if (!text || sending) return;

    // No-op in Tauri mode — add user message locally for visual feedback
    const userMsg = { role: 'user', text, timestamp: Date.now() / 1000 };
    setMessages((prev) => [...prev, userMsg]);
    setInput('');
  }, [input, sending]);

  const handleKeyDown = useCallback((e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }, [handleSend]);

  return (
    <div className="border border-titan-border rounded-lg overflow-hidden">
      <div className="flex items-center gap-2 px-3 py-2 bg-titan-surface/50 border-b border-titan-border">
        <MessageCircle size={14} className="text-blue-400" />
        <span className="text-xs font-medium text-titan-text-muted">
          Consciousness Dialogue
        </span>
        <span className="ml-auto text-[10px] text-titan-text-muted/50">
          persistent thread
        </span>
      </div>

      <div ref={scrollRef} className="max-h-64 overflow-y-auto p-3 space-y-2">
        {messages.length === 0 && (
          <div className="flex items-center justify-center h-24 text-xs text-titan-text-muted">
            No messages yet — start a conversation with consciousness
          </div>
        )}
        {messages.map((msg, i) => (
          <div
            key={msg.timestamp + '-' + i}
            className={`flex ${msg.role === 'user' ? 'justify-end' : 'justify-start'}`}
          >
            <div
              className={`max-w-[80%] rounded-lg px-3 py-2 text-xs ${
                msg.role === 'user'
                  ? 'bg-blue-600/20 text-blue-200'
                  : 'bg-purple-600/20 text-purple-200'
              }`}
            >
              <p className="leading-relaxed">{msg.text}</p>
              <span className="text-[10px] opacity-50 mt-1 block">
                {msg.role === 'consciousness' ? 'Titan' : 'You'} — {timeAgo(msg.timestamp)}
              </span>
            </div>
          </div>
        ))}
      </div>

      <div className="flex items-center gap-2 p-2 border-t border-titan-border">
        <input
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Reply to consciousness..."
          className="flex-1 bg-titan-surface/30 text-xs text-titan-text rounded px-2 py-1.5 border border-titan-border focus:outline-none focus:border-blue-500"
          disabled={sending}
        />
        <button
          onClick={handleSend}
          disabled={sending || !input.trim()}
          className="p-1.5 rounded bg-blue-600/20 text-blue-400 hover:bg-blue-600/30 disabled:opacity-30 transition-colors"
        >
          <Send size={12} />
        </button>
      </div>
    </div>
  );
}
