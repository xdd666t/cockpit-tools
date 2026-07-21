import { listWorkbuddyAccounts, getCheckinStatusWorkbuddy, checkinWorkbuddy } from './workbuddyService';
import { useWorkbuddyAccountStore } from '../stores/useWorkbuddyAccountStore';

export interface WorkbuddyAutoCheckinConfig {
  enabled: boolean;
  startTime: string; // HH:mm, e.g. "06:00"
  endTime: string;   // HH:mm, e.g. "12:00"
  lastCheckedDate?: string; // "YYYY-MM-DD"
  scheduledDateToday?: string; // "YYYY-MM-DD"
  scheduledMinuteToday?: number; // Minutes from midnight (0..1439)
}

export const DEFAULT_WORKBUDDY_AUTO_CHECKIN_CONFIG: WorkbuddyAutoCheckinConfig = {
  // 签到会对所有已保存账号发起远程请求，必须由用户显式开启。
  enabled: false,
  startTime: '06:00',
  endTime: '12:00',
};

const CONFIG_KEY = 'agtools.workbuddy.auto_checkin_config';
export const WORKBUDDY_AUTO_CHECKIN_CONFIG_CHANGED_EVENT = 'workbuddy-auto-checkin-config-changed';
const AUTO_CHECKIN_RETRY_DELAY_MS = 5 * 60 * 1000;
const AUTO_CHECKIN_IDLE_RECHECK_DELAY_MS = 60 * 60 * 1000;

export type WorkbuddyAutoCheckinCycleResult = 'disabled' | 'waiting' | 'completed' | 'retry';

function isValidTime(time: unknown): time is string {
  return typeof time === 'string' && /^([01]\d|2[0-3]):[0-5]\d$/.test(time);
}

export function getWorkbuddyAutoCheckinConfig(): WorkbuddyAutoCheckinConfig {
  if (typeof window === 'undefined') {
    return DEFAULT_WORKBUDDY_AUTO_CHECKIN_CONFIG;
  }
  try {
    const raw = localStorage.getItem(CONFIG_KEY);
    if (!raw) {
      return DEFAULT_WORKBUDDY_AUTO_CHECKIN_CONFIG;
    }
    const parsed = JSON.parse(raw);
    return {
      enabled: typeof parsed.enabled === 'boolean' ? parsed.enabled : false,
      startTime: isValidTime(parsed.startTime) ? parsed.startTime : '06:00',
      endTime: isValidTime(parsed.endTime) ? parsed.endTime : '12:00',
      lastCheckedDate: typeof parsed.lastCheckedDate === 'string' ? parsed.lastCheckedDate : undefined,
      scheduledDateToday: typeof parsed.scheduledDateToday === 'string' ? parsed.scheduledDateToday : undefined,
      scheduledMinuteToday: typeof parsed.scheduledMinuteToday === 'number' ? parsed.scheduledMinuteToday : undefined,
    };
  } catch {
    return DEFAULT_WORKBUDDY_AUTO_CHECKIN_CONFIG;
  }
}

export function saveWorkbuddyAutoCheckinConfig(config: WorkbuddyAutoCheckinConfig): void {
  if (typeof window === 'undefined') {
    return;
  }
  try {
    localStorage.setItem(CONFIG_KEY, JSON.stringify(config));
    window.dispatchEvent(new Event(WORKBUDDY_AUTO_CHECKIN_CONFIG_CHANGED_EVENT));
  } catch (err) {
    console.warn('[WorkbuddyAutoCheckin] 保存配置失败:', err);
  }
}

export function parseTimeToMinutes(timeStr: string): number {
  const parts = timeStr.split(':').map(Number);
  const h = parts[0] ?? 0;
  const m = parts[1] ?? 0;
  return h * 60 + m;
}

export function formatMinutesToTime(minutes: number): string {
  const h = Math.floor(minutes / 60) % 24;
  const m = minutes % 60;
  return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`;
}

export function getTodayDateString(): string {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

export function ensureTodayScheduledMinute(config: WorkbuddyAutoCheckinConfig): {
  updatedConfig: WorkbuddyAutoCheckinConfig;
  scheduledMinute: number;
} {
  const todayStr = getTodayDateString();
  const startMin = parseTimeToMinutes(config.startTime);
  let endMin = parseTimeToMinutes(config.endTime);
  if (endMin < startMin) {
    endMin = startMin;
  }

  if (
    config.scheduledDateToday === todayStr &&
    typeof config.scheduledMinuteToday === 'number' &&
    config.scheduledMinuteToday >= startMin &&
    config.scheduledMinuteToday <= endMin
  ) {
    return { updatedConfig: config, scheduledMinute: config.scheduledMinuteToday };
  }

  const minRange = Math.max(0, endMin - startMin);
  const randomOffset = minRange > 0 ? Math.floor(Math.random() * (minRange + 1)) : 0;
  const scheduledMinute = startMin + randomOffset;

  const updatedConfig: WorkbuddyAutoCheckinConfig = {
    ...config,
    scheduledDateToday: todayStr,
    scheduledMinuteToday: scheduledMinute,
  };

  saveWorkbuddyAutoCheckinConfig(updatedConfig);
  return { updatedConfig, scheduledMinute };
}

let isAutoCheckinCycleRunning = false;

function getMillisecondsUntilNextLocalDay(now: Date): number {
  const nextDay = new Date(now);
  nextDay.setHours(24, 0, 1, 0);
  return Math.max(1_000, nextDay.getTime() - now.getTime());
}

export function getWorkbuddyAutoCheckinNextDelayMs(
  result?: WorkbuddyAutoCheckinCycleResult,
): number {
  const config = getWorkbuddyAutoCheckinConfig();
  if (!config.enabled) {
    return AUTO_CHECKIN_IDLE_RECHECK_DELAY_MS;
  }

  const now = new Date();
  const todayStr = getTodayDateString();
  if (config.lastCheckedDate === todayStr) {
    return getMillisecondsUntilNextLocalDay(now);
  }

  if (result === 'retry') {
    return AUTO_CHECKIN_RETRY_DELAY_MS;
  }

  const { scheduledMinute } = ensureTodayScheduledMinute(config);
  const scheduledAt = new Date(now);
  scheduledAt.setHours(Math.floor(scheduledMinute / 60), scheduledMinute % 60, 0, 0);
  return Math.max(1_000, scheduledAt.getTime() - now.getTime());
}

export interface WorkbuddyAutoCheckinAccountDetail {
  accountId: string;
  email: string;
  status: 'success' | 'already_checked' | 'failed' | 'inactive';
  message?: string;
  credit?: number;
}

export interface WorkbuddyAutoCheckinLogRecord {
  id: string;
  timestamp: string; // formatted e.g. "YYYY-MM-DD HH:mm:ss"
  date: string;      // "YYYY-MM-DD"
  durationMs: number;
  totalAccounts: number;
  successCount: number;
  alreadyCheckedCount: number;
  failedCount: number;
  status: 'success' | 'partial' | 'failed' | 'no_accounts';
  details: WorkbuddyAutoCheckinAccountDetail[];
}

const LOGS_KEY = 'agtools.workbuddy.auto_checkin_logs';
export const WORKBUDDY_AUTO_CHECKIN_LOGS_CHANGED_EVENT = 'workbuddy-auto-checkin-logs-changed';
const THIRTY_DAYS_MS = 30 * 24 * 60 * 60 * 1000;

export function getWorkbuddyAutoCheckinLogs(): WorkbuddyAutoCheckinLogRecord[] {
  if (typeof window === 'undefined') {
    return [];
  }
  try {
    const raw = localStorage.getItem(LOGS_KEY);
    if (!raw) {
      return [];
    }
    const parsed = JSON.parse(raw) as WorkbuddyAutoCheckinLogRecord[];
    if (!Array.isArray(parsed)) {
      return [];
    }

    const now = Date.now();
    // 自动过滤并清理超过 30 天的历史记录
    const validLogs = parsed.filter((log) => {
      const logTime = new Date(log.timestamp.replace(' ', 'T')).getTime();
      return !isNaN(logTime) && now - logTime <= THIRTY_DAYS_MS;
    });

    if (validLogs.length !== parsed.length) {
      localStorage.setItem(LOGS_KEY, JSON.stringify(validLogs));
    }
    return validLogs;
  } catch {
    return [];
  }
}

export function saveWorkbuddyAutoCheckinLogs(logs: WorkbuddyAutoCheckinLogRecord[]): void {
  if (typeof window === 'undefined') {
    return;
  }
  try {
    const now = Date.now();
    // 仅保留近 30 天记录
    const validLogs = logs.filter((log) => {
      const logTime = new Date(log.timestamp.replace(' ', 'T')).getTime();
      return !isNaN(logTime) && now - logTime <= THIRTY_DAYS_MS;
    });
    localStorage.setItem(LOGS_KEY, JSON.stringify(validLogs));
    window.dispatchEvent(new Event(WORKBUDDY_AUTO_CHECKIN_LOGS_CHANGED_EVENT));
  } catch (err) {
    console.warn('[WorkbuddyAutoCheckin] 保存自动签到日志失败:', err);
  }
}

export function addWorkbuddyAutoCheckinLog(record: WorkbuddyAutoCheckinLogRecord): void {
  const currentLogs = getWorkbuddyAutoCheckinLogs();
  saveWorkbuddyAutoCheckinLogs([record, ...currentLogs]);
}

export function clearWorkbuddyAutoCheckinLogs(): void {
  saveWorkbuddyAutoCheckinLogs([]);
}

export function formatFormattedTimestamp(date: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(
    date.getHours(),
  )}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

export async function runWorkbuddyAutoCheckinCycleIfNeeded(
  force = false,
): Promise<WorkbuddyAutoCheckinCycleResult> {
  const config = getWorkbuddyAutoCheckinConfig();
  if (!config.enabled && !force) {
    return 'disabled';
  }

  const todayStr = getTodayDateString();
  if (!force && config.lastCheckedDate === todayStr) {
    return 'completed';
  }

  const { scheduledMinute } = ensureTodayScheduledMinute(config);
  const now = new Date();
  const currentMinute = now.getHours() * 60 + now.getMinutes();

  if (!force && currentMinute < scheduledMinute) {
    return 'waiting';
  }

  if (isAutoCheckinCycleRunning) {
    return 'waiting';
  }

  isAutoCheckinCycleRunning = true;
  const startTime = Date.now();
  const startTimestampStr = formatFormattedTimestamp(new Date(startTime));

  try {
    console.log('[WorkbuddyAutoCheckin] 开始触发自动签到流程...');
    const accounts = await listWorkbuddyAccounts();
    if (accounts.length === 0) {
      if (!force) {
        saveWorkbuddyAutoCheckinConfig({
          ...getWorkbuddyAutoCheckinConfig(),
          lastCheckedDate: todayStr,
        });
      }
      addWorkbuddyAutoCheckinLog({
        id: `log_${startTime}_${Math.random().toString(36).substring(2, 7)}`,
        timestamp: startTimestampStr,
        date: todayStr,
        durationMs: Date.now() - startTime,
        totalAccounts: 0,
        successCount: 0,
        alreadyCheckedCount: 0,
        failedCount: 0,
        status: 'no_accounts',
        details: [],
      });
      return 'completed';
    }

    let didCheckinAny = false;
    let retryNeeded = false;
    let successCount = 0;
    let alreadyCheckedCount = 0;
    let failedCount = 0;
    const details: WorkbuddyAutoCheckinAccountDetail[] = [];

    for (const account of accounts) {
      const emailDisplay = account.email || account.id;
      try {
        const status = await getCheckinStatusWorkbuddy(account.id);
        if (status.today_checked_in) {
          alreadyCheckedCount++;
          details.push({
            accountId: account.id,
            email: emailDisplay,
            status: 'already_checked',
            message: '今日已完成签到',
            credit: status.daily_credit ?? undefined,
          });
        } else if (status.active === false) {
          details.push({
            accountId: account.id,
            email: emailDisplay,
            status: 'inactive',
            message: '签到活动未开启或不适用',
          });
        } else {
          console.log(`[WorkbuddyAutoCheckin] 为账号 ${emailDisplay} 执行自动签到...`);
          const res = await checkinWorkbuddy(account.id);
          if (res.success) {
            didCheckinAny = true;
            successCount++;
            details.push({
              accountId: account.id,
              email: emailDisplay,
              status: 'success',
              message: res.message || '签到成功',
              credit: res.credit ?? undefined,
            });
          } else {
            // 可能是另一处流程已签到；复查后再决定是否需要重试
            const latestStatus = await getCheckinStatusWorkbuddy(account.id);
            if (latestStatus.today_checked_in) {
              alreadyCheckedCount++;
              details.push({
                accountId: account.id,
                email: emailDisplay,
                status: 'already_checked',
                message: '今日已完成签到',
              });
            } else {
              retryNeeded = true;
              failedCount++;
              details.push({
                accountId: account.id,
                email: emailDisplay,
                status: 'failed',
                message: res.message || '签到失败',
              });
            }
          }
        }
      } catch (accountErr) {
        const errMsg = accountErr instanceof Error ? accountErr.message : String(accountErr);
        console.warn(`[WorkbuddyAutoCheckin] 账号 ${account.id} 签到检测异常:`, accountErr);
        retryNeeded = true;
        failedCount++;
        details.push({
          accountId: account.id,
          email: emailDisplay,
          status: 'failed',
          message: errMsg,
        });
      }
    }

    const durationMs = Date.now() - startTime;
    const overallStatus: WorkbuddyAutoCheckinLogRecord['status'] =
      failedCount === 0
        ? 'success'
        : successCount > 0 || alreadyCheckedCount > 0
          ? 'partial'
          : 'failed';

    addWorkbuddyAutoCheckinLog({
      id: `log_${startTime}_${Math.random().toString(36).substring(2, 7)}`,
      timestamp: startTimestampStr,
      date: todayStr,
      durationMs,
      totalAccounts: accounts.length,
      successCount,
      alreadyCheckedCount,
      failedCount,
      status: overallStatus,
      details,
    });

    if (!retryNeeded && !force) {
      saveWorkbuddyAutoCheckinConfig({
        ...getWorkbuddyAutoCheckinConfig(),
        lastCheckedDate: todayStr,
      });
    }
    if (didCheckinAny) {
      void useWorkbuddyAccountStore.getState().fetchAccounts().catch((err) => {
        console.warn('[WorkbuddyAutoCheckin] 刷新账号列表失败:', err);
      });
    }
    return retryNeeded ? 'retry' : 'completed';
  } catch (err) {
    console.error('[WorkbuddyAutoCheckin] 自动签到周期失败:', err);
    return 'retry';
  } finally {
    isAutoCheckinCycleRunning = false;
  }
}
