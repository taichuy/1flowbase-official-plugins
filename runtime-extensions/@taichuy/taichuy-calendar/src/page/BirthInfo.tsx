import React from 'react';
import { Card, Form, DatePicker, TimePicker, Radio, Button, Tooltip, InputNumber, Switch, Typography } from 'antd';
import { InfoCircleOutlined } from '@ant-design/icons';
import type { Dayjs } from 'dayjs';
import { Solar } from 'lunar-typescript';
import LocationSelector from '../components/LocationSelector';
import { usePluginContext } from '../state/PluginContext';
import { usePageStyles } from './styles';

export default function BirthInfoForm() {
  const { styles, cx } = usePageStyles();
  const { birthDate, setBirthDate, time, setTime, gender, setGender, useTrueSolarTime,
    setUseTrueSolarTime, longitude, setLongitude, latitude, setLatitude, locationCodes,
    setLocationCodes, setLocationName, trueSolarDateTime, requestChart } = usePluginContext();
  const cellRender = (
    current: Dayjs | number | string,
    info: { originNode: React.ReactElement; type: string }
  ) => {
    if (info.type !== 'date') return info.originNode;
    const date = current as Dayjs;
    const d = Solar.fromYmd(date.year(), date.month() + 1, date.date());
    const lunar = d.getLunar();
    const lunarText = lunar.getDayInChinese();
    const solarTerm = lunar.getJieQi();

    return (
      <div className={cx((info.originNode.props as { className?: string }).className, styles.dateCell)}>
        <span className={styles.solarDate}>{date.date()}</span>
        <span className={styles.lunarDate}>{solarTerm || lunarText}</span>
      </div>
    );
  };

  return (
          <Card data-calendar-section="birth-info" className={styles.formCard} variant="borderless">
            <Form layout="vertical">
              <div className={styles.formRow}>
                <Form.Item label="出生日期">
                  <DatePicker
                    aria-label="出生日期"
                    value={birthDate}
                    onChange={(d) => d && setBirthDate(d)}
                    style={{ width: '100%' }}
                    allowClear={false}
                    cellRender={cellRender}
                    classNames={{ popup: { root: styles.datePickerPopup } }}
                    inputReadOnly
                  />
                </Form.Item>
                <Form.Item label="出生时间">
                  <TimePicker
                    aria-label="出生时间"
                    value={time}
                    onChange={(t) => t && setTime(t)}
                    format="HH:mm"
                    style={{ width: '100%' }}
                    allowClear={false}
                  />
                </Form.Item>
              </div>

              <div className={styles.formRow}>
                <Form.Item label="出生地点" style={{ flex: 1.5 }}>
                  <LocationSelector
                    value={locationCodes}
                    onChange={(codes, lng, lat, labels) => {
                      setLocationCodes(codes);
                      if (lng !== null) setLongitude(lng);
                      if (lat !== null) setLatitude(lat);
                      if (labels) setLocationName(labels.join(' '));
                    }}
                  />
                </Form.Item>
                <Form.Item label="经度" style={{ flex: 0.8 }}>
                  <InputNumber
                    aria-label="经度"
                    value={longitude}
                    onChange={(val) => {
                      if (val !== null) setLongitude(val);
                      if (locationCodes.length > 0) {
                        setLocationCodes([]);
                        setLocationName(`自定义 (${val?.toFixed(2)}, ${latitude.toFixed(2)})`);
                      }
                    }}
                    step={0.01}
                    style={{ width: '100%' }}
                  />
                </Form.Item>
                <Form.Item label="纬度" style={{ flex: 0.8 }}>
                  <InputNumber
                    aria-label="纬度"
                    value={latitude}
                    onChange={(val) => {
                      if (val !== null) setLatitude(val);
                      if (locationCodes.length > 0) {
                        setLocationCodes([]);
                        setLocationName(`自定义 (${longitude.toFixed(2)}, ${val?.toFixed(2)})`);
                      }
                    }}
                    step={0.01}
                    style={{ width: '100%' }}
                  />
                </Form.Item>
                <Form.Item
                  label={
                    <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                      <span>真太阳时</span>
                      <Tooltip title="根据经纬度和均时差校正出生时间，排盘更准确">
                        <InfoCircleOutlined style={{ color: '#999' }} />
                      </Tooltip>
                    </div>
                  }
                  style={{ flex: 1.2 }}
                >
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, height: 32 }}>
                    <Switch aria-label="真太阳时" checked={useTrueSolarTime} onChange={setUseTrueSolarTime} />
                    {useTrueSolarTime && (
                      <Typography.Text
                        type="secondary"
                        style={{ fontSize: 12, whiteSpace: 'nowrap' }}
                      >
                        {trueSolarDateTime.time.format('HH:mm')}
                      </Typography.Text>
                    )}
                  </div>
                </Form.Item>
              </div>

              <div className={styles.formRow}>
                <Form.Item label="性别">
                  <Radio.Group value={gender} onChange={(e) => setGender(e.target.value)}>
                    <Radio value="male">男</Radio>
                    <Radio value="female">女</Radio>
                  </Radio.Group>
                </Form.Item>
                <Form.Item style={{ flex: '0 0 auto' }}>
                  <Button type="primary" onClick={requestChart}>开始排盘</Button>
                </Form.Item>
              </div>
            </Form>
          </Card>
  );
}
