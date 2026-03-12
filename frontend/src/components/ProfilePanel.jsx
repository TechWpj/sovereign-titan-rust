import { useState, useEffect, useCallback } from 'react';
import { User, ChevronDown, ChevronRight, Eye, EyeOff, Plus, Trash2, Lock, LogOut, Shield } from 'lucide-react';
import { getProfile, updateProfile } from '../api/client';

const EMPTY_ACCOUNT = { site: '', username: '', password: '', notes: '' };
const SESSION_KEY = 'sovereign-titan-profile-key';

export default function ProfilePanel() {
  const [collapsed, setCollapsed] = useState(true);
  const [adminKey, setAdminKey] = useState(() => sessionStorage.getItem(SESSION_KEY) || '');
  const [unlocked, setUnlocked] = useState(false);
  const [keyInput, setKeyInput] = useState('');
  const [authError, setAuthError] = useState('');
  const [profile, setProfile] = useState(null);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [showSSN, setShowSSN] = useState(false);
  const [showPasswords, setShowPasswords] = useState({});
  const [showGooglePwd, setShowGooglePwd] = useState(false);
  const [showPin, setShowPin] = useState(false);
  const [showAdminPwd, setShowAdminPwd] = useState(false);

  const load = useCallback(async (key) => {
    try {
      const data = await getProfile(key);
      setProfile(data);
      setUnlocked(true);
      setAuthError('');
      setError('');
    } catch (e) {
      if (e.message === 'unauthorized') {
        setUnlocked(false);
        setAdminKey('');
        sessionStorage.removeItem(SESSION_KEY);
        setAuthError('Invalid admin key');
      } else {
        setProfile(null);
        setError(e.message);
      }
    }
  }, []);

  // Try auto-unlock if key is in sessionStorage
  useEffect(() => {
    if (!collapsed && adminKey && !unlocked) {
      load(adminKey);
    }
  }, [collapsed, adminKey, unlocked, load]);

  async function handleUnlock(e) {
    e.preventDefault();
    if (!keyInput.trim()) return;
    setAuthError('');
    try {
      await getProfile(keyInput.trim());
      setAdminKey(keyInput.trim());
      sessionStorage.setItem(SESSION_KEY, keyInput.trim());
      setUnlocked(true);
      load(keyInput.trim());
    } catch (err) {
      if (err.message === 'unauthorized') {
        setAuthError('Invalid admin key');
      } else {
        setAuthError(err.message);
      }
    }
  }

  function handleLock() {
    setUnlocked(false);
    setAdminKey('');
    setKeyInput('');
    setProfile(null);
    setDirty(false);
    sessionStorage.removeItem(SESSION_KEY);
  }

  function update(field, value) {
    setProfile((p) => ({ ...p, [field]: value }));
    setDirty(true);
  }

  function updateAddress(field, value) {
    setProfile((p) => ({
      ...p,
      address: { ...p.address, [field]: value },
    }));
    setDirty(true);
  }

  function updateAdminCreds(field, value) {
    setProfile((p) => ({
      ...p,
      admin_credentials: { ...(p.admin_credentials || {}), [field]: value },
    }));
    setDirty(true);
  }

  function updateAccount(idx, field, value) {
    setProfile((p) => {
      const accts = [...p.accounts];
      accts[idx] = { ...accts[idx], [field]: value };
      return { ...p, accounts: accts };
    });
    setDirty(true);
  }

  function addAccount() {
    setProfile((p) => ({
      ...p,
      accounts: [...(p.accounts || []), { ...EMPTY_ACCOUNT }],
    }));
    setDirty(true);
  }

  function removeAccount(idx) {
    setProfile((p) => ({
      ...p,
      accounts: p.accounts.filter((_, i) => i !== idx),
    }));
    setDirty(true);
  }

  async function handleSave() {
    setSaving(true);
    setError('');
    try {
      const updated = await updateProfile(profile, adminKey);
      setProfile(updated);
      setDirty(false);
    } catch (e) {
      if (e.message === 'unauthorized') {
        handleLock();
        setAuthError('Session expired — re-enter admin key');
      } else {
        setError(e.message);
      }
    } finally {
      setSaving(false);
    }
  }

  const inputCls =
    'w-full rounded border border-titan-border bg-titan-surface px-2 py-1 text-[11px] text-titan-text placeholder-titan-text-muted/50 outline-none focus:border-titan-accent';

  return (
    <div className="border-t border-titan-border">
      <button
        onClick={() => setCollapsed(!collapsed)}
        className="flex w-full items-center gap-2 px-3 py-2 text-xs font-medium text-titan-text-muted hover:text-titan-text transition-colors"
      >
        {unlocked ? (
          <User size={13} className="text-violet-400" />
        ) : (
          <Lock size={13} className="text-violet-400" />
        )}
        <span>Profile & Credentials</span>
        <span className="ml-auto">
          {collapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
        </span>
      </button>

      {!collapsed && !unlocked && (
        <div className="px-3 pb-3 space-y-2">
          <p className="text-[10px] text-titan-text-muted/70">
            Enter the admin key shown at server launch to access your profile.
          </p>
          <form onSubmit={handleUnlock} className="space-y-1.5">
            <input
              className={inputCls}
              type="password"
              placeholder="Admin Key"
              value={keyInput}
              onChange={(e) => { setKeyInput(e.target.value); setAuthError(''); }}
              autoFocus
            />
            <button
              type="submit"
              className="w-full rounded-lg bg-violet-600 px-3 py-1.5 text-[11px] font-medium text-white transition-colors hover:bg-violet-500"
            >
              Unlock
            </button>
            {authError && (
              <p className="text-[10px] text-red-400">{authError}</p>
            )}
          </form>
        </div>
      )}

      {!collapsed && unlocked && (
        <div className="px-3 pb-2 space-y-2 max-h-80 overflow-y-auto">
          {/* Lock button */}
          <div className="flex justify-end">
            <button
              onClick={handleLock}
              className="flex items-center gap-1 text-[10px] text-titan-text-muted hover:text-red-400 transition-colors"
              title="Lock profile"
            >
              <LogOut size={10} />
              Lock
            </button>
          </div>

          {!profile ? (
            <p className="text-[10px] text-titan-text-muted/60">Loading profile...</p>
          ) : (
            <>
              {/* Personal */}
              <div className="space-y-1.5">
                <span className="text-[10px] font-medium text-titan-text-muted uppercase tracking-wider">
                  Personal
                </span>
                <input
                  className={inputCls}
                  placeholder="Full Name"
                  value={profile.name || ''}
                  onChange={(e) => update('name', e.target.value)}
                />
                <input
                  className={inputCls}
                  placeholder="Email"
                  type="email"
                  value={profile.email || ''}
                  onChange={(e) => update('email', e.target.value)}
                />
                <input
                  className={inputCls}
                  placeholder="Phone"
                  value={profile.phone || ''}
                  onChange={(e) => update('phone', e.target.value)}
                />
                <input
                  className={inputCls}
                  placeholder="Date of Birth"
                  value={profile.date_of_birth || ''}
                  onChange={(e) => update('date_of_birth', e.target.value)}
                />
                <div className="relative">
                  <input
                    className={inputCls + ' pr-7'}
                    placeholder="SSN"
                    type={showSSN ? 'text' : 'password'}
                    value={profile.ssn || ''}
                    onChange={(e) => update('ssn', e.target.value)}
                  />
                  <button
                    onClick={() => setShowSSN(!showSSN)}
                    className="absolute right-1.5 top-1/2 -translate-y-1/2 text-titan-text-muted hover:text-titan-text"
                  >
                    {showSSN ? <EyeOff size={11} /> : <Eye size={11} />}
                  </button>
                </div>
              </div>

              {/* Address */}
              <div className="space-y-1.5">
                <span className="text-[10px] font-medium text-titan-text-muted uppercase tracking-wider">
                  Address
                </span>
                <input
                  className={inputCls}
                  placeholder="Street"
                  value={profile.address?.street || ''}
                  onChange={(e) => updateAddress('street', e.target.value)}
                />
                <div className="flex gap-1.5">
                  <input
                    className={inputCls}
                    placeholder="City"
                    value={profile.address?.city || ''}
                    onChange={(e) => updateAddress('city', e.target.value)}
                  />
                  <input
                    className={inputCls + ' w-16 shrink-0'}
                    placeholder="State"
                    value={profile.address?.state || ''}
                    onChange={(e) => updateAddress('state', e.target.value)}
                  />
                </div>
                <input
                  className={inputCls + ' w-24'}
                  placeholder="ZIP"
                  value={profile.address?.zip_code || ''}
                  onChange={(e) => updateAddress('zip_code', e.target.value)}
                />
              </div>

              {/* System / Automation */}
              <div className="space-y-1.5">
                <span className="text-[10px] font-medium text-titan-text-muted uppercase tracking-wider">
                  System & Automation
                </span>
                <input
                  className={inputCls}
                  placeholder="Google Account (email for Chrome & OAuth)"
                  type="email"
                  value={profile.google_account || ''}
                  onChange={(e) => update('google_account', e.target.value)}
                />
                <div className="relative">
                  <input
                    className={inputCls + ' pr-7'}
                    placeholder="Google Account Password"
                    type={showGooglePwd ? 'text' : 'password'}
                    value={profile.google_password || ''}
                    onChange={(e) => update('google_password', e.target.value)}
                  />
                  <button
                    onClick={() => setShowGooglePwd(!showGooglePwd)}
                    className="absolute right-1.5 top-1/2 -translate-y-1/2 text-titan-text-muted hover:text-titan-text"
                  >
                    {showGooglePwd ? <EyeOff size={11} /> : <Eye size={11} />}
                  </button>
                </div>
                <div className="relative">
                  <input
                    className={inputCls + ' pr-7'}
                    placeholder="System PIN (password manager, Windows Hello)"
                    type={showPin ? 'text' : 'password'}
                    value={profile.system_pin || ''}
                    onChange={(e) => update('system_pin', e.target.value)}
                  />
                  <button
                    onClick={() => setShowPin(!showPin)}
                    className="absolute right-1.5 top-1/2 -translate-y-1/2 text-titan-text-muted hover:text-titan-text"
                  >
                    {showPin ? <EyeOff size={11} /> : <Eye size={11} />}
                  </button>
                </div>
                <div className="rounded border border-titan-border/50 p-1.5 space-y-1">
                  <div className="flex items-center gap-1 text-[10px] text-titan-text-muted">
                    <Shield size={10} />
                    Admin Credentials
                  </div>
                  <input
                    className={inputCls + ' text-[10px]'}
                    placeholder="Windows Username"
                    value={profile.admin_credentials?.username || ''}
                    onChange={(e) => updateAdminCreds('username', e.target.value)}
                  />
                  <div className="relative">
                    <input
                      className={inputCls + ' pr-7 text-[10px]'}
                      placeholder="Windows Password"
                      type={showAdminPwd ? 'text' : 'password'}
                      value={profile.admin_credentials?.password || ''}
                      onChange={(e) => updateAdminCreds('password', e.target.value)}
                    />
                    <button
                      onClick={() => setShowAdminPwd(!showAdminPwd)}
                      className="absolute right-1.5 top-1/2 -translate-y-1/2 text-titan-text-muted hover:text-titan-text"
                    >
                      {showAdminPwd ? <EyeOff size={11} /> : <Eye size={11} />}
                    </button>
                  </div>
                  <input
                    className={inputCls + ' text-[10px]'}
                    placeholder="Notes (e.g. domain, other types)"
                    value={profile.admin_credentials?.notes || ''}
                    onChange={(e) => updateAdminCreds('notes', e.target.value)}
                  />
                </div>
              </div>

              {/* Accounts */}
              <div className="space-y-1.5">
                <div className="flex items-center justify-between">
                  <span className="text-[10px] font-medium text-titan-text-muted uppercase tracking-wider">
                    Accounts
                  </span>
                  <button
                    onClick={addAccount}
                    className="flex items-center gap-0.5 text-[10px] text-cyan-400 hover:text-cyan-300 transition-colors"
                  >
                    <Plus size={10} />
                    Add
                  </button>
                </div>
                {(profile.accounts || []).map((acct, idx) => (
                  <div
                    key={idx}
                    className="space-y-1 rounded border border-titan-border/50 p-1.5"
                  >
                    <div className="flex items-center justify-between">
                      <input
                        className={inputCls + ' text-[10px]'}
                        placeholder="Site (e.g. github.com)"
                        value={acct.site || ''}
                        onChange={(e) => updateAccount(idx, 'site', e.target.value)}
                      />
                      <button
                        onClick={() => removeAccount(idx)}
                        className="ml-1 shrink-0 text-titan-text-muted hover:text-red-400"
                      >
                        <Trash2 size={11} />
                      </button>
                    </div>
                    <input
                      className={inputCls + ' text-[10px]'}
                      placeholder="Username"
                      value={acct.username || ''}
                      onChange={(e) => updateAccount(idx, 'username', e.target.value)}
                    />
                    <div className="relative">
                      <input
                        className={inputCls + ' pr-7 text-[10px]'}
                        placeholder="Password"
                        type={showPasswords[idx] ? 'text' : 'password'}
                        value={acct.password || ''}
                        onChange={(e) => updateAccount(idx, 'password', e.target.value)}
                      />
                      <button
                        onClick={() =>
                          setShowPasswords((p) => ({ ...p, [idx]: !p[idx] }))
                        }
                        className="absolute right-1.5 top-1/2 -translate-y-1/2 text-titan-text-muted hover:text-titan-text"
                      >
                        {showPasswords[idx] ? (
                          <EyeOff size={11} />
                        ) : (
                          <Eye size={11} />
                        )}
                      </button>
                    </div>
                    <input
                      className={inputCls + ' text-[10px]'}
                      placeholder="Notes (optional)"
                      value={acct.notes || ''}
                      onChange={(e) => updateAccount(idx, 'notes', e.target.value)}
                    />
                  </div>
                ))}
              </div>

              {/* Save */}
              {dirty && (
                <button
                  onClick={handleSave}
                  disabled={saving}
                  className="w-full rounded-lg bg-titan-accent px-3 py-1.5 text-[11px] font-medium text-white transition-colors hover:bg-titan-accent/80 disabled:opacity-50"
                >
                  {saving ? 'Saving...' : 'Save Profile'}
                </button>
              )}
              {error && (
                <p className="text-[10px] text-red-400">{error}</p>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
