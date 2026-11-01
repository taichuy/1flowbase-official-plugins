import React, { createContext, useContext } from 'react';
import zh from './zh.json';
import en from './en.json';

export type Locale = 'zh' | 'en';
type TranslationKey = keyof typeof zh;
const dictionaries = { zh, en };
const LocaleContext = createContext<Locale>('zh');

export function I18nProvider({ locale, children }: { locale: Locale; children: React.ReactNode }) {
  return <LocaleContext.Provider value={locale}>{children}</LocaleContext.Provider>;
}

export function useT() {
  const locale = useContext(LocaleContext);
  return (key: TranslationKey) => dictionaries[locale][key];
}
