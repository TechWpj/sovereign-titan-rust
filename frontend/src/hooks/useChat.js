import { useState, useRef, useCallback, useEffect } from 'react';
import { chatStream, fetchTools } from '../api/client';
import { DEFAULT_MODEL, DEEP_MODEL, SYSTEM_PROMPT } from '../lib/constants';

function buildSystemPrompt(tools) {
  if (!tools.length) {
    return SYSTEM_PROMPT.replace('{TOOLS}', '(Tools loading...)');
  }
  const toolLines = tools.map((t) => {
    const fn = t.function || t;
    const params = fn.parameters?.properties
      ? Object.entries(fn.parameters.properties)
          .map(([k, v]) => `${k}: ${v.description || v.type}`)
          .join(', ')
      : '';
    return `- ${fn.name}(${params}): ${fn.description}`;
  });
  return SYSTEM_PROMPT.replace('{TOOLS}', toolLines.join('\n'));
}

export function useChat({ addMessage, updateLastMessage, addToolCallToLastMessage, addPhaseToLastMessage, addThinkingToLastMessage }) {
  const [isStreaming, setIsStreaming] = useState(false);
  const isStreamingRef = useRef(false);
  const [deepMode, setDeepMode] = useState(false);
  const abortRef = useRef(null);
  const toolsRef = useRef([]);

  useEffect(() => {
    fetchTools().then((t) => { toolsRef.current = t; });
  }, []);

  const sendMessage = useCallback(
    async (content, conversation) => {
      if (!conversation || isStreamingRef.current) return;

      const convId = conversation.id;

      const userMsg = {
        id: crypto.randomUUID(),
        role: 'user',
        content,
        timestamp: Date.now(),
      };
      addMessage(convId, userMsg);

      const systemMsg = {
        role: 'system',
        content: buildSystemPrompt(toolsRef.current),
      };

      const apiMessages = [
        systemMsg,
        ...conversation.messages.map((m) => ({
          role: m.role,
          content: m.content,
        })),
        { role: 'user', content },
      ];

      const assistantMsg = {
        id: crypto.randomUUID(),
        role: 'assistant',
        content: '',
        timestamp: Date.now(),
      };
      addMessage(convId, assistantMsg);

      setIsStreaming(true);
      isStreamingRef.current = true;
      const controller = new AbortController();
      abortRef.current = controller;

      try {
        const activeModel = deepMode ? DEEP_MODEL : DEFAULT_MODEL;
        const stream = chatStream(apiMessages, activeModel, controller.signal);
        for await (const event of stream) {
          if (event.type === 'token') {
            updateLastMessage(convId, (prev) => prev + event.content);
          } else if (
            event.type === 'phase_start' ||
            event.type === 'phase_complete' ||
            event.type === 'synthesis_start'
          ) {
            if (addPhaseToLastMessage) {
              addPhaseToLastMessage(convId, event);
            }
          } else if (
            event.type === 'thought' ||
            event.type === 'tool_call' ||
            event.type === 'tool_result'
          ) {
            addToolCallToLastMessage(convId, event);
          } else if (event.type === 'thinking') {
            if (addThinkingToLastMessage) {
              addThinkingToLastMessage(convId, event.content);
            }
          }
        }
      } catch (err) {
        if (err.name !== 'AbortError') {
          updateLastMessage(convId, (prev) =>
            prev + (prev ? '\n\n' : '') + `[Error: ${err.message}]`,
          );
        }
      } finally {
        isStreamingRef.current = false;
        setIsStreaming(false);
        abortRef.current = null;
      }
    },
    [deepMode, addMessage, updateLastMessage, addToolCallToLastMessage, addPhaseToLastMessage, addThinkingToLastMessage],
  );

  const stopStreaming = useCallback(() => {
    abortRef.current?.abort();
  }, []);

  const toggleDeepMode = useCallback(() => {
    setDeepMode((prev) => !prev);
  }, []);

  return { isStreaming, deepMode, sendMessage, stopStreaming, toggleDeepMode };
}
