import type { ReactElement } from 'react';

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
export declare function Calendar(props?: CalendarComponentProps): ReactElement;
export declare function Almanac(props?: CalendarComponentProps): ReactElement;
export declare function BirthInfo(props?: CalendarComponentProps): ReactElement;
export declare function ZiWei(props?: CalendarComponentProps): ReactElement;
export declare function Bazi(props?: CalendarComponentProps): ReactElement;
export declare function CalendarPage(props?: CalendarComponentProps): ReactElement;
