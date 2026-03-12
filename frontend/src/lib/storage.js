import { STORAGE_KEYS } from './constants';

export function getConversations() {
  try {
    const raw = localStorage.getItem(STORAGE_KEYS.CONVERSATIONS);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

export function saveConversations(conversations) {
  localStorage.setItem(STORAGE_KEYS.CONVERSATIONS, JSON.stringify(conversations));
}

export function getApiKey() {
  return localStorage.getItem(STORAGE_KEYS.API_KEY) || '';
}

export function saveApiKey(key) {
  if (key) {
    localStorage.setItem(STORAGE_KEYS.API_KEY, key);
  } else {
    localStorage.removeItem(STORAGE_KEYS.API_KEY);
  }
}
