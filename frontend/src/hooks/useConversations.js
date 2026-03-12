import { useState, useCallback } from 'react';
import { getConversations, saveConversations } from '../lib/storage';
import { MAX_TITLE_LENGTH } from '../lib/constants';

export function useConversations() {
  const [conversations, setConversations] = useState(() => getConversations());
  const [activeId, setActiveId] = useState(null);

  const persist = useCallback((next) => {
    setConversations(next);
    saveConversations(next);
  }, []);

  const createConversation = useCallback(() => {
    const conv = {
      id: crypto.randomUUID(),
      title: 'New Chat',
      messages: [],
      createdAt: Date.now(),
      updatedAt: Date.now(),
    };
    persist([conv, ...conversations]);
    setActiveId(conv.id);
    return conv;
  }, [conversations, persist]);

  const deleteConversation = useCallback((id) => {
    const next = conversations.filter((c) => c.id !== id);
    persist(next);
    if (activeId === id) setActiveId(next[0]?.id || null);
  }, [conversations, activeId, persist]);

  const updateConversation = useCallback((id, updates) => {
    const next = conversations.map((c) =>
      c.id === id ? { ...c, ...updates, updatedAt: Date.now() } : c,
    );
    persist(next);
  }, [conversations, persist]);

  const addMessage = useCallback((convId, message) => {
    setConversations((prev) => {
      const next = prev.map((c) => {
        if (c.id !== convId) return c;
        const messages = [...c.messages, message];
        const title =
          c.messages.length === 0 && message.role === 'user'
            ? message.content.slice(0, MAX_TITLE_LENGTH)
            : c.title;
        return { ...c, messages, title, updatedAt: Date.now() };
      });
      saveConversations(next);
      return next;
    });
  }, []);

  const updateLastMessage = useCallback((convId, contentOrUpdater) => {
    setConversations((prev) => {
      const next = prev.map((c) => {
        if (c.id !== convId) return c;
        const msgs = [...c.messages];
        const last = msgs[msgs.length - 1];
        if (!last) return c;
        const newContent =
          typeof contentOrUpdater === 'function'
            ? contentOrUpdater(last.content)
            : contentOrUpdater;
        msgs[msgs.length - 1] = { ...last, content: newContent };
        return { ...c, messages: msgs, updatedAt: Date.now() };
      });
      saveConversations(next);
      return next;
    });
  }, []);

  const addToolCallToLastMessage = useCallback((convId, toolCallData) => {
    setConversations((prev) => {
      const next = prev.map((c) => {
        if (c.id !== convId) return c;
        const msgs = [...c.messages];
        const last = msgs[msgs.length - 1];
        if (!last) return c;
        const toolCalls = [...(last.toolCalls || []), toolCallData];
        msgs[msgs.length - 1] = { ...last, toolCalls };
        return { ...c, messages: msgs, updatedAt: Date.now() };
      });
      saveConversations(next);
      return next;
    });
  }, []);

  const addPhaseToLastMessage = useCallback((convId, phaseData) => {
    setConversations((prev) => {
      const next = prev.map((c) => {
        if (c.id !== convId) return c;
        const msgs = [...c.messages];
        const last = msgs[msgs.length - 1];
        if (!last) return c;
        const phases = [...(last.phases || []), phaseData];
        msgs[msgs.length - 1] = { ...last, phases };
        return { ...c, messages: msgs, updatedAt: Date.now() };
      });
      saveConversations(next);
      return next;
    });
  }, []);

  const addThinkingToLastMessage = useCallback((convId, text) => {
    setConversations((prev) => {
      const next = prev.map((c) => {
        if (c.id !== convId) return c;
        const msgs = [...c.messages];
        const last = msgs[msgs.length - 1];
        if (!last) return c;
        const thinking = (last.thinking || '') + text;
        msgs[msgs.length - 1] = { ...last, thinking };
        return { ...c, messages: msgs, updatedAt: Date.now() };
      });
      saveConversations(next);
      return next;
    });
  }, []);

  const activeConversation = conversations.find((c) => c.id === activeId) || null;

  return {
    conversations,
    activeId,
    activeConversation,
    setActiveId,
    createConversation,
    deleteConversation,
    updateConversation,
    addMessage,
    updateLastMessage,
    addToolCallToLastMessage,
    addPhaseToLastMessage,
    addThinkingToLastMessage,
  };
}
