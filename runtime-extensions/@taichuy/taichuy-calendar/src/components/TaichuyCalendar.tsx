'use client';
import React, { useImperativeHandle, useRef, useState, forwardRef } from 'react';
import { Calendar, Row, Col, Select, Radio } from 'antd';
import { createStyles } from 'antd-style';
import { clsx } from 'clsx';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';
import { Lunar } from 'lunar-typescript';
import type { CalendarProps } from 'antd';
import { useDateContext } from '../state/DateContext';

type TaichuyCalendarProps = object;

export type TaichuyCalendarHandles = {
  goToday: () => void;
};

const useStyle = createStyles(({ token, css, cx }) => {
  const lunar = css`
    color: ${token.colorTextTertiary};
    font-size: ${token.fontSizeSM}px;
  `;
  const weekend = css`
    color: ${token.colorError};
    &.gray {
      opacity: 0.4;
    }
  `;
  return {
    wrapper: css`
      width: 100%;
      border: 1px solid ${token.colorBorderSecondary};
      border-radius: ${token.borderRadiusOuter}px;
      padding: 5px;
    `,
    dateCell: css`
      position: relative;
      &:before {
        content: '';
        position: absolute;
        inset-inline-start: 0;
        inset-inline-end: 0;
        top: 0;
        bottom: 0;
        margin: auto;
        max-width: 40px;
        max-height: 40px;
        background: transparent;
        transition: background-color 300ms;
        border-radius: ${token.borderRadiusOuter}px;
        border: 1px solid transparent;
        box-sizing: border-box;
      }
      &:hover:before {
        background: rgba(0, 0, 0, 0.04);
      }
    `,
    today: css`
      &:before {
        border: 1px solid ${token.colorPrimary};
      }
    `,
    text: css`
      position: relative;
      z-index: 1;
    `,
    lunar,
    current: css`
      color: inherit;
      &:before {
        background: ${token.colorPrimary};
        opacity: 0.3;
      }
      &:hover:before {
        background: ${token.colorPrimary};
        opacity: 0.3;
      }
      .${cx(lunar)} {
        color: ${token.colorTextTertiary};
      }
      .${cx(weekend)} {
        color: ${token.colorError};
      }
    `,
    monthCell: css`
      width: 96px;
      color: ${token.colorTextBase};
      border-radius: ${token.borderRadiusOuter}px;
      padding: 4px 8px;
      display: inline-block;
      text-align: center;
      position: relative;
      &:hover {
        background: rgba(0, 0, 0, 0.04);
      }
    `,
    monthCellCurrent: css`
      color: ${token.colorTextLightSolid};
      position: relative;
      &:before {
        content: '';
        position: absolute;
        inset: 0;
        background: ${token.colorPrimary};
        opacity: 0.3;
        border-radius: ${token.borderRadiusOuter}px;
      }
      &:hover:before {
        background: ${token.colorPrimary};
        opacity: 0.3;
      }
    `,
    monthSolar: css`
      position: relative;
      z-index: 1;
      font-size: 14px;
      line-height: 20px;
    `,
    monthLunar: css`
      position: relative;
      z-index: 1;
      font-size: 12px;
      color: ${token.colorTextTertiary};
      line-height: 18px;
    `,
    weekend,
  };
});

const TaichuyCalendar = forwardRef<TaichuyCalendarHandles, TaichuyCalendarProps>((_, ref) => {
  const { styles, cx } = useStyle();
  const { selectedDate: selectDate, setSelectedDate: setSelectDate } = useDateContext();
  const typeChangeRef = useRef<((t: 'month' | 'year') => void) | undefined>(undefined);
  const panelChangeRef = useRef<((v: Dayjs) => void) | undefined>(undefined);
  const [calendarType, setCalendarType] = useState<'month' | 'year'>('month');
  const [panelDate, setPanelDate] = useState<Dayjs>(() => selectDate);

  const goToday = () => {
    const now = dayjs();
    setSelectDate(now);
    setPanelDate(now);
    setCalendarType('month');
    typeChangeRef.current?.('month');
    setTimeout(() => {
      typeChangeRef.current?.('month');
      panelChangeRef.current?.(now);
    }, 0);
  };

  useImperativeHandle(ref, () => ({ goToday }));

  const onPanelChange = (value: Dayjs) => {
    setPanelDate(value);
  };
  const onDateChange = (value: Dayjs, selectInfo: { source?: 'date' | 'month' | string }) => {
    if (selectInfo.source === 'date') {
      setSelectDate(value);
      setPanelDate(value);
    } else if (selectInfo.source === 'month') {
      setSelectDate(value);
      setPanelDate(value);
    }
  };

  const fullCellRender: NonNullable<CalendarProps<Dayjs>['fullCellRender']> = (date, info) => {
    const d = Lunar.fromDate(date.toDate());
    const lunarText = d.getDayInChinese();
    const solarTerm = d.getJieQi();
    const isWeekend = date.day() === 6 || date.day() === 0;

    if (info.type === 'date') {
      const key = date.format('YYYY-MM-DD');
      return (
        <div
          data-calendar-date={key}
          className={clsx(styles.dateCell, {
            [styles.current]: selectDate.isSame(date, 'date'),
            [styles.today]: date.isSame(dayjs(), 'date'),
          })}
        >
          <div className={styles.text} style={{ position: 'relative' }}>
            <span
              className={clsx({
                [styles.weekend]: isWeekend,
                gray: !panelDate.isSame(date, 'month'),
              })}
            >
              {date.get('date')}
            </span>
            <div className={styles.lunar}>{solarTerm || lunarText}</div>

          </div>
        </div>
      );
    }

    if (info.type === 'month') {
      const d2 = Lunar.fromDate(new Date(date.get('year'), date.get('month')));
      const month = d2.getMonthInChinese();
      return (
        <div
          className={clsx(styles.monthCell, {
            [styles.monthCellCurrent]: selectDate.isSame(date, 'month'),
          })}
          onClick={() => {
            setSelectDate(date);
            setPanelDate(date);
            typeChangeRef.current?.('month');
          }}
        >
          <div className={styles.monthSolar}>{date.get('month') + 1}月</div>
          <div className={styles.monthLunar}>{month}月</div>
        </div>
      );
    }
  };

  return (
    <div data-calendar-section="calendar" data-calendar-mode={calendarType} className={styles.wrapper}>
      <Calendar
        fullscreen={false}
        value={panelDate}
        mode={calendarType}
        key={`${calendarType}-${panelDate.format('YYYY-MM')}`}
        fullCellRender={fullCellRender}
        onPanelChange={onPanelChange}
        onSelect={onDateChange}
        headerRender={({ value, onChange, onTypeChange }) => {
          typeChangeRef.current = onTypeChange;
          panelChangeRef.current = onChange;
          const start = 0;
          const end = 12;
          const monthOptions: Array<{ label: string; value: number }> = [];
          for (let i = start; i < end; i++) {
            monthOptions.push({
              label: `${i + 1}月（${Lunar.fromDate(new Date(value.year(), i)).getMonthInChinese()}月）`,
              value: i,
            });
          }
          const year = value.year();
          const month = value.month();
          const options: Array<{ label: string; value: number }> = [];
          for (let i = year - 10; i < year + 10; i += 1) {
            const d = Lunar.fromDate(new Date(i + 1, 0));
            options.push({
              label: `${d.getYearInChinese()}年（${d.getYearInGanZhi()}${d.getYearShengXiao()}年）`,
              value: i,
            });
          }
          return (
            <Row justify="end" gutter={8} style={{ padding: 8 }}>
              <Col>
                <Select
                  size="small"
                  popupMatchSelectWidth={false}
                  aria-label="日历年份"
                  className="my-year-select"
                  value={year}
                  options={options}
                  onChange={(newYear) => {
                    const now = value.clone().year(newYear);
                    onChange(now);
                    setPanelDate(now);
                  }}
                />
              </Col>
              <Col>
                <Select
                  size="small"
                  popupMatchSelectWidth={false}
                  aria-label="日历月份"
                  value={month}
                  options={monthOptions}
                  onChange={(newMonth) => {
                    const now = value.clone().month(newMonth);
                    onChange(now);
                    setPanelDate(now);
                  }}
                />
              </Col>
              <Col>
                <Radio.Group
                  size="small"
                  onChange={(e) => {
                    const v = e.target.value as 'month' | 'year';
                    setCalendarType(v);
                    onTypeChange(v);
                  }}
                  value={calendarType}
                >
                  <Radio.Button value="month">月</Radio.Button>
                  <Radio.Button value="year">年</Radio.Button>
                </Radio.Group>
              </Col>
            </Row>
          );
        }}
      />
    </div>
  );
});

TaichuyCalendar.displayName = 'TaichuyCalendar';

export default TaichuyCalendar;
