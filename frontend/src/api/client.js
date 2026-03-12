import { invoke } from '@tauri-apps/api/core';

// --- Chat streaming (non-streaming via IPC, preserves async generator interface) ---

export async function* chatStream(messages, model, signal) {
  const lastUserMsg = [...messages].reverse().find((m) => m.role === 'user');
  const message = lastUserMsg?.content || '';

  const response = await invoke('send_chat', { message });
  yield { type: 'token', content: response };
}

// --- Models ---

export async function fetchModels() {
  try {
    const statuses = await invoke('get_status');
    return statuses.map((s) => ({ id: s.name, loaded: s.loaded }));
  } catch {
    return [];
  }
}

export async function fetchTools() {
  return [];
}

// --- Health ---

export async function checkHealth() {
  try {
    await invoke('get_status');
    return true;
  } catch {
    return false;
  }
}

// --- Voice (stubs) ---

export async function speakText(text) {
  return new Blob();
}

export async function transcribeAudio(audioBlob) {
  return '';
}

export async function fetchVoiceStatus() {
  return { continuous_listening: false, auto_respond: false, wake_word: 'titan' };
}

export async function toggleVoiceListen() {
  return {};
}

export async function fetchVoiceEvents() {
  return [];
}

export async function speakVoiceResponse(text) {
  return {};
}

export async function stopVoiceSpeaking() {
  return {};
}

export async function updateVoiceSettings(settings) {
  return {};
}

export async function toggleVoiceAutoRespond() {
  return {};
}

// --- Tasks (stubs) ---

export async function submitTask(description) {
  return { id: crypto.randomUUID(), description, status: 'queued' };
}

export async function listTasks(status) {
  return [];
}

export async function cancelTask(taskId) {
  return {};
}

// --- Consciousness (stubs) ---

export async function getThoughts(n = 10) {
  return [];
}

export async function getConsciousnessLedgers() {
  return {};
}

export async function getConsciousnessModes() {
  return { mode_weights: {}, mode_history: [] };
}

// --- Profile (stubs) ---

export async function getProfile(adminKey) {
  return {};
}

export async function updateProfile(profile, adminKey) {
  return {};
}

// --- Automation (stubs) ---

export async function startAutomation(description, { headless = false } = {}) {
  return {};
}

export async function confirmAutomation(taskId, fieldOverrides) {
  return {};
}

export async function cancelAutomation(taskId) {
  return {};
}

export async function listAutomations() {
  return { tasks: [] };
}

// --- Security (stubs) ---

export async function getSecurityEvents(level = null, limit = 100) {
  return { events: [] };
}

export async function triggerSecurityScan() {
  return {};
}

export async function getSecurityReport() {
  return {};
}
