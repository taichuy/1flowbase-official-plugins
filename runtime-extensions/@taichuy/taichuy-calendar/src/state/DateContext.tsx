'use client';
import React, { createContext, useState, useContext } from 'react';
import dayjs, { Dayjs } from 'dayjs';

interface DateContextType {
  selectedDate: Dayjs;
  setSelectedDate: (date: Dayjs) => void;
}

const DateContext = createContext<DateContextType | undefined>(undefined);

export const DateProvider = ({ children, initialDate }: { children: React.ReactNode; initialDate?: string }) => {
  const [selectedDate, setSelectedDate] = useState<Dayjs>(() => dayjs(initialDate).locale('zh-cn'));

  return (
    <DateContext.Provider value={{ selectedDate, setSelectedDate }}>
      {children}
    </DateContext.Provider>
  );
};

export const useDateContext = () => {
  const context = useContext(DateContext);
  if (!context) {
    throw new Error('useDateContext must be used within a DateProvider');
  }
  return context;
};
