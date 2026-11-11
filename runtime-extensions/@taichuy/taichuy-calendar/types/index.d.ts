import type { ReactElement, ReactNode, Dispatch, SetStateAction } from 'react';
import type { Dayjs } from 'dayjs';

export interface CalendarComponentProps {
  /** Initial calendar/almanac date, independent of the birth date. ISO civil date. */
  initialDate?: string;
  initialBirthDate?: string;
  /** HH:mm or HH:mm:ss, interpreted in the browser's timezone. */
  initialTime?: string;
  initialGender?: 'male' | 'female';
  initialLongitude?: number;
  initialLatitude?: number;
  initialUseTrueSolarTime?: boolean;
  /** Changes the source's quotation/disclaimer copy; traditional chart labels stay Chinese. */
  locale?: 'zh' | 'en';
}
/** Replaces the whole assistant panel; chart calculation and navigation remain plugin-owned.
 * These strings are the same complete descriptions used by the built-in copy actions.
 * They are browser render data, not persisted records or an external MCP query result.
 */
export interface ZiWeiAIHelperData {
  kind: 'ziwei';
  natalText: string;
  horoscopeText: string;
  /** Selected horoscope civil date/time, YYYY-MM-DD HH:mm:ss. */
  viewDate: string;
  copyNatal: () => void;
  copyHoroscope: () => void;
}
export interface BaziAIHelperData {
  kind: 'bazi';
  natalText: string;
  selectedDaYunIndex: number;
  copyNatal: () => void;
}
export type ChartAIHelperData = ZiWeiAIHelperData | BaziAIHelperData;
/** Return null to omit the panel. Omit the prop to keep the original assistant. */
export type ChartAIHelperRenderer = (data: ChartAIHelperData) => ReactNode;
export interface ChartComponentProps extends CalendarComponentProps {
  renderAIHelper?: ChartAIHelperRenderer;
}
export declare function Calendar(props?: CalendarComponentProps): ReactElement;
export declare function Almanac(props?: CalendarComponentProps): ReactElement;
export declare function BirthInfo(props?: CalendarComponentProps): ReactElement;
export declare function ZiWei(props?: ChartComponentProps): ReactElement;
export declare function Bazi(props?: ChartComponentProps): ReactElement;
export declare function CalendarPage(props?: CalendarComponentProps): ReactElement;

/** Owns an isolated calendar + birth state tree. Wrap sibling components to link them. */
export declare function CalendarProvider(props: CalendarComponentProps & { children: ReactNode }): ReactElement;
export interface CalendarDateState {
  selectedDate: Dayjs;
  setSelectedDate: (date: Dayjs) => void;
}
export declare function useCalendarDate(): CalendarDateState;
export interface ChartLocationInfo {
  location?: string;
  longitude: number;
  latitude: number;
  useTrueSolarTime: boolean;
}
export interface BirthInfoState {
  birthDate: Dayjs;
  setBirthDate: Dispatch<SetStateAction<Dayjs>>;
  time: Dayjs;
  setTime: Dispatch<SetStateAction<Dayjs>>;
  gender: 'male' | 'female';
  setGender: Dispatch<SetStateAction<'male' | 'female'>>;
  useTrueSolarTime: boolean;
  setUseTrueSolarTime: Dispatch<SetStateAction<boolean>>;
  longitude: number;
  setLongitude: Dispatch<SetStateAction<number>>;
  latitude: number;
  setLatitude: Dispatch<SetStateAction<number>>;
  locationCodes: string[];
  setLocationCodes: Dispatch<SetStateAction<string[]>>;
  locationName: string;
  setLocationName: Dispatch<SetStateAction<string>>;
  trueSolarDateTime: { date: Dayjs; time: Dayjs };
  locationInfo: ChartLocationInfo;
  chartRevision: number;
  requestChart: () => void;
}
export declare function useBirthInfo(): BirthInfoState;
export interface ChartProps {
  date: Dayjs;
  time: Dayjs;
  /** Selected civil birth timestamp for helper text, before true solar correction.
   * Omit when date/time already represent the selected civil input.
   */
  civilDateTime?: Dayjs;
  gender: 'male' | 'female';
  locationInfo?: ChartLocationInfo;
  renderAIHelper?: ChartAIHelperRenderer;
}
export declare function LunarInfoCard(props: { date: Dayjs }): ReactElement;
export declare function ZiWeiCalculator(props: ChartProps): ReactElement;
export declare function BaziCalculator(props: ChartProps): ReactElement;
export declare function LocationSelector(props: {
  value?: string[];
  onChange?: (value: string[], lng: number | null, lat: number | null, labels?: string[]) => void;
}): ReactElement;
