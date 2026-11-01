import React, { createContext, useContext, useMemo, useState } from 'react';
import dayjs, { type Dayjs } from 'dayjs';
import { App, ConfigProvider } from 'antd';
import { ThemeProvider } from 'antd-style';
import zhCN from 'antd/locale/zh_CN';
import 'dayjs/locale/zh-cn';
import { DateProvider } from './DateContext';
import { I18nProvider, type Locale } from '../i18n/client';
import { calculateTrueSolarTime } from '../algorithms/true-solar-time';

export interface CalendarComponentProps {
  /** Almanac/calendar selection; deliberately independent of birthDate, as in the source. */
  initialDate?: string;
  initialBirthDate?: string;
  /** Local civil time HH:mm or HH:mm:ss, interpreted in the browser's timezone. */
  initialTime?: string;
  initialGender?: 'male' | 'female';
  initialLongitude?: number;
  initialLatitude?: number;
  initialUseTrueSolarTime?: boolean;
  /** Source quote/disclaimer translations; the traditional charts retain Chinese labels. */
  locale?: Locale;
}

function useBirthState(props: CalendarComponentProps) {
  const [birthDate, setBirthDate] = useState<Dayjs>(() => dayjs(props.initialBirthDate).locale('zh-cn'));
  const [time, setTime] = useState<Dayjs>(() => props.initialTime
    ? dayjs(`2000-01-01T${props.initialTime}`).locale('zh-cn') : dayjs().locale('zh-cn'));
  const [gender, setGender] = useState<'male' | 'female'>(props.initialGender ?? 'male');
  const [useTrueSolarTime, setUseTrueSolarTime] = useState(props.initialUseTrueSolarTime ?? true);
  const [longitude, setLongitude] = useState(props.initialLongitude ?? 116.4);
  const [latitude, setLatitude] = useState(props.initialLatitude ?? 39.9);
  const [locationCodes, setLocationCodes] = useState<string[]>([]);
  const [locationName, setLocationName] = useState(() => longitude === 116.4 && latitude === 39.9 ? '北京市 北京市' : `自定义 (${longitude.toFixed(2)}, ${latitude.toFixed(2)})`);
  const [chartRevision, setChartRevision] = useState(0);
  const trueSolarDateTime = useMemo(() => {
    if (!useTrueSolarTime) return { date: birthDate.clone(), time: time.clone() };
    const adjusted = calculateTrueSolarTime(birthDate, time, longitude, latitude);
    return { date: adjusted, time: adjusted };
  }, [birthDate, time, longitude, latitude, useTrueSolarTime]);
  const locationInfo = useMemo(() => ({ location: locationName, longitude, latitude, useTrueSolarTime }),
    [locationName, longitude, latitude, useTrueSolarTime]);
  return { birthDate, setBirthDate, time, setTime, gender, setGender,
    useTrueSolarTime, setUseTrueSolarTime, longitude, setLongitude, latitude, setLatitude,
    locationCodes, setLocationCodes, locationName, setLocationName, trueSolarDateTime,
    locationInfo, chartRevision, requestChart: () => setChartRevision((n) => n + 1) };
}

type PluginState = ReturnType<typeof useBirthState>;
const PluginContext = createContext<PluginState | null>(null);

export function usePluginContext(): PluginState {
  const state = useContext(PluginContext);
  if (!state) throw new Error('Calendar plugin provider is required');
  return state;
}

export function CalendarProvider({ children, ...props }: CalendarComponentProps & { children: React.ReactNode }) {
  const state = useBirthState(props);
  return (
    <ThemeProvider themeMode="light" theme={{ token: {
      colorPrimary: '#0056D2', colorInfo: '#0056D2', colorLink: '#0056D2',
      colorBgLayout: '#f0f2f5', colorTextBase: '#0f172a'
    } }}>
    <ConfigProvider locale={zhCN}>
      <App>
        <I18nProvider locale={props.locale ?? 'zh'}>
          <DateProvider initialDate={props.initialDate}>
            <PluginContext.Provider value={state}>{children}</PluginContext.Provider>
          </DateProvider>
        </I18nProvider>
      </App>
    </ConfigProvider>
    </ThemeProvider>
  );
}

/** Standalone exports own state; an explicit CalendarProvider shares state across composed children. */
export function PluginBoundary({ children, ...props }: CalendarComponentProps & { children: React.ReactNode }) {
  const parent = useContext(PluginContext);
  return parent ? <>{children}</> : <CalendarProvider {...props}>{children}</CalendarProvider>;
}
