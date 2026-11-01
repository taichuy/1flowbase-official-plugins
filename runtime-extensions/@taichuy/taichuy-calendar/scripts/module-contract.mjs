export const moduleSource = '@taichuy/taichuy-calendar';
export const componentExports = ['Calendar', 'Almanac', 'BirthInfo', 'ZiWei', 'Bazi', 'CalendarPage'];
export const publicExports = [...componentExports, 'CalendarProvider', 'useBirthInfo', 'useCalendarDate', 'LunarInfoCard', 'ZiWeiCalculator', 'BaziCalculator', 'LocationSelector'];
export const businessDependencies = {
  'lunar-typescript': '1.8.6',
  tyme4ts: '1.3.9',
  iztro: '2.5.3',
  'astronomy-engine': '2.1.19',
  'china-division': '2.7.0',
};

// Values are ESM-like namespaces, including .default where declared. No globalThis bridge.
export const hostModules = [
  { source: 'react', version: '19.2.5', exports: ['default', 'createContext', 'useContext', 'useState', 'useMemo', 'useEffect', 'useRef', 'useImperativeHandle', 'forwardRef'] },
  { source: 'antd', version: '6.6.2', exports: ['App', 'Button', 'Calendar', 'Card', 'Cascader', 'Col', 'ConfigProvider', 'DatePicker', 'Descriptions', 'Flex', 'Form', 'Input', 'InputNumber', 'Radio', 'Row', 'Select', 'Switch', 'Table', 'Tabs', 'Tag', 'TimePicker', 'Tooltip', 'Typography', 'theme'] },
  { source: 'dayjs', version: '1.11.20', exports: ['default'] },
  { source: '@ant-design/icons', version: '6.3.4', exports: ['CheckOutlined', 'CompassOutlined', 'CalculatorOutlined', 'InfoCircleOutlined', 'CopyOutlined', 'LeftOutlined', 'RightOutlined', 'FileTextOutlined', 'QuestionCircleOutlined', 'ReloadOutlined'] },
  { source: 'antd-style', version: '4.1.0', exports: ['createStyles', 'ThemeProvider'] },
  { source: 'clsx', version: '2.1.1', exports: ['clsx'] },
];

// Existing source-site support code, NOT additional host ABI requirements or new downloads.
export const bundledSupportDependencies = { 'copy-to-clipboard': '3.3.3' };

/** Only Chinese locale metadata may cross the otherwise external-only antd/dayjs boundary. */
export function isBundledHostMetadata(input) {
  return /(?:^|\/)node_modules\/antd\/(?:(?:lib|es)\/)?(?:locale|(?:calendar|date-picker|time-picker)\/locale)\/zh_CN\.js$/.test(input)
    || /(?:^|\/)node_modules\/dayjs\/locale\/zh-cn\.js$/.test(input);
}

export function assertBundlingBoundary(inputs) {
  for (const input of inputs) {
    if (/(?:^|\/)node_modules\/(?:react|react-dom|antd|dayjs|antd-style|clsx|@ant-design\/icons)(?:\/|$)/.test(input)
      && !isBundledHostMetadata(input)) {
      throw new Error(`Host module was bundled: ${input}`);
    }
  }
}
