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

export async function runWorkbuddyAutoCheckinCycleIfNeeded(): Promise<WorkbuddyAutoCheckinCycleResult> {
  const config = getWorkbuddyAutoCheckinConfig();
  if (!config.enabled) {
    return 'disabled';
  }

  const todayStr = getTodayDateString();
  if (config.lastCheckedDate === todayStr) {
    return 'completed';
  }

  const { scheduledMinute } = ensureTodayScheduledMinute(config);
  const now = new Date();
  const currentMinute = now.getHours() * 60 + now.getMinutes();

  if (currentMinute < scheduledMinute) {
    return 'waiting';
  }

  if (isAutoCheckinCycleRunning) {
    return 'waiting';
  }

  isAutoCheckinCycleRunning = true;
  try {
    console.log('[WorkbuddyAutoCheckin] 开始触发自动签到流程...');
    const accounts = await listWorkbuddyAccounts();
    if (accounts.length === 0) {
      saveWorkbuddyAutoCheckinConfig({ ...getWorkbuddyAutoCheckinConfig(), lastCheckedDate: todayStr });
      return 'completed';
    }

    let didCheckinAny = false;
    let retryNeeded = false;
    for (const account of accounts) {
      try {
        const status = await getCheckinStatusWorkbuddy(account.id);
        if (!status.today_checked_in && status.active !== false) {
          console.log(`[WorkbuddyAutoCheckin] 为账号 ${account.email || account.id} 执行自动签到...`);
          const res = await checkinWorkbuddy(account.id);
          if (res.success) {
            didCheckinAny = true;
          } else {
            // 可能是另一处流程已签到；复查后再决定是否需要重试。
            const latestStatus = await getCheckinStatusWorkbuddy(account.id);
            if (!latestStatus.today_checked_in && latestStatus.active !== false) {
              retryNeeded = true;
            }
          }
        }
      } catch (accountErr) {
        console.warn(`[WorkbuddyAutoCheckin] 账号 ${account.id} 签到检测异常:`, accountErr);
        retryNeeded = true;
      }
    }

    if (!retryNeeded) {
      saveWorkbuddyAutoCheckinConfig({ ...getWorkbuddyAutoCheckinConfig(), lastCheckedDate: todayStr });
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
