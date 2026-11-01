export const moduleSource = '@taichuy/taichuy-calendar';
export const componentExports = ['Calendar', 'Almanac', 'BirthInfo', 'ZiWei', 'Bazi', 'CalendarPage'];
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
  { source: 'antd-style', version: '4.1.0', exports: ['createStyles'] },
  { source: 'clsx', version: '2.1.1', exports: ['clsx'] },
  { source: 'copy-to-clipboard', version: '4.0.2', exports: ['default'] },
  { source: 'antd/locale/zh_CN', version: '6.6.2', exports: ['default'] },
  { source: 'dayjs/locale/zh-cn', version: '1.11.20', exports: ['default'] },
];
