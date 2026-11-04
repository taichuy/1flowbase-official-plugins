'use client';
import React, { useRef } from 'react';
import { Card, Button, Typography } from 'antd';
import { createStyles } from 'antd-style';
import TaichuyCalendar, { TaichuyCalendarHandles } from './TaichuyCalendar';
const SYSTEM_NAME = '太初y';
import { useT } from '../i18n/client';

const { Title, Text } = Typography;

const useStyle = createStyles(({ css }) => {
  return {
    card: css`
      box-shadow: 0 1px 2px 0 rgba(0, 0, 0, 0.05);
      border: 1px solid #e2e8f0;
    `,
    subtitle: css`
      font-size: 12px;
      color: #64748b;
    `,
    btnGroup: css`
      display: flex;
      gap: 8px;
      margin-top: 12px;
    `,
    todayBtn: css`
      background-color: #f6ffed;
      color: #52c41a;
      border-color: #b7eb8f;
      font-weight: bold;
      font-size: 12px;
      &:hover {
        background-color: #d9f7be !important;
        border-color: #95de64 !important;
        color: #389e0d !important;
      }
    `,
  };
});

interface TaichuyCalendarCardProps {
  className?: string;
  style?: React.CSSProperties;
}

export default function TaichuyCalendarCard({ className, style }: TaichuyCalendarCardProps) {
  const { styles, cx } = useStyle();
  const t = useT();
  const calendarRef = useRef<TaichuyCalendarHandles>(null);

  return (
    <Card
      variant="outlined"
      className={cx(styles.card, className)}
      style={{ borderTop: '3px solid #0056D2', ...style }}
      styles={{ body: { padding: '12px 16px' } }}
    >
      <Title
        level={4}
        style={{
          marginBottom: '2px',
          fontFamily: '"Times New Roman", serif',
          fontStyle: 'italic',
          color: '#0f172a',
          fontSize: '1.125rem',
          lineHeight: '1.75rem',
        }}
      >
        {SYSTEM_NAME}
      </Title>
      <Text className={styles.subtitle}>{t('rightRail.subtitle')}</Text>
      <TaichuyCalendar ref={calendarRef} />
      <div className={styles.btnGroup}>
        <Button className={styles.todayBtn} onClick={() => calendarRef.current?.goToday()}>
          今天
        </Button>
      </div>
    </Card>
  );
}
