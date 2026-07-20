import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { X, Clock, AlertCircle } from 'lucide-react';
import { useEscClose } from '../../hooks/useEscClose';
import {
  WorkbuddyAutoCheckinConfig,
  parseTimeToMinutes,
} from '../../services/workbuddyAutoCheckinService';

interface WorkbuddyAutoCheckinConfigModalProps {
  config: WorkbuddyAutoCheckinConfig;
  onSave: (newConfig: WorkbuddyAutoCheckinConfig) => void;
  onClose: () => void;
}

export function WorkbuddyAutoCheckinConfigModal({
  config,
  onSave,
  onClose,
}: WorkbuddyAutoCheckinConfigModalProps) {
  const { t } = useTranslation();
  useEscClose(true, onClose);

  const [enabled, setEnabled] = useState(config.enabled);
  const [startTime, setStartTime] = useState(config.startTime || '06:00');
  const [endTime, setEndTime] = useState(config.endTime || '12:00');
  const [error, setError] = useState<string | null>(null);

  const handleSave = () => {
    const startMin = parseTimeToMinutes(startTime);
    const endMin = parseTimeToMinutes(endTime);

    if (startMin > endMin) {
      setError(t('workbuddy.checkin.timeRangeError', '开始时间不能晚于结束时间'));
      return;
    }

    onSave({
      ...config,
      enabled,
      startTime,
      endTime,
      // Clear today's scheduled minute if time range changed so it recalculates
      scheduledMinuteToday: undefined,
      scheduledDateToday: undefined,
    });
    onClose();
  };

  return (
    <div className="modal-overlay auto-checkin-config-overlay" onClick={onClose}>
      <div
        className="modal-content auto-checkin-config-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-header">
          <h2>
            <Clock size={18} />
            {t('workbuddy.checkin.autoCheckinSettings', '自动签到设置')}
          </h2>
          <button className="modal-close" onClick={onClose}>
            <X size={18} />
          </button>
        </div>

        <div className="modal-body auto-checkin-config-body">
          <div className="config-form-item toggle-item">
            <div className="toggle-label">
              <span className="label-title">
                {t('workbuddy.checkin.enableAutoCheckin', '开启自动签到')}
              </span>
              <span className="label-desc">
                {t(
                  'workbuddy.checkin.enableAutoCheckinDesc',
                  '每天在指定时间段内随机选取一个时间完成自动签到',
                )}
              </span>
            </div>
            <label className="toggle-switch">
              <input
                type="checkbox"
                checked={enabled}
                onChange={(e) => setEnabled(e.target.checked)}
              />
              <span className="slider round"></span>
            </label>
          </div>

          <div className={`config-form-section ${!enabled ? 'disabled' : ''}`}>
            <h3 className="section-title">
              {t('workbuddy.checkin.randomTimeRange', '随机签到时间段')}
            </h3>

            <div className="time-range-inputs">
              <div className="time-input-group">
                <label>{t('workbuddy.checkin.startTime', '开始时间')}</label>
                <input
                  type="time"
                  value={startTime}
                  onChange={(e) => {
                    setStartTime(e.target.value);
                    setError(null);
                  }}
                  disabled={!enabled}
                />
              </div>

              <span className="time-separator">{t('common.to', '至')}</span>

              <div className="time-input-group">
                <label>{t('workbuddy.checkin.endTime', '结束时间')}</label>
                <input
                  type="time"
                  value={endTime}
                  onChange={(e) => {
                    setEndTime(e.target.value);
                    setError(null);
                  }}
                  disabled={!enabled}
                />
              </div>
            </div>

            <p className="section-hint">
              {t(
                'workbuddy.checkin.timeRangeHint',
                '在这个时间段内随机选取一个时间点完成后台自动签到。系统将自动保持运行并检测。',
              )}
            </p>
          </div>

          {error && (
            <div className="config-error-message">
              <AlertCircle size={14} /> {error}
            </div>
          )}
        </div>

        <div className="modal-footer">
          <button className="btn btn-secondary" onClick={onClose}>
            {t('common.cancel', '取消')}
          </button>
          <button className="btn btn-primary" onClick={handleSave}>
            {t('common.save', '保存')}
          </button>
        </div>
      </div>
    </div>
  );
}
