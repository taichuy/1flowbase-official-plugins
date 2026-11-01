'use client';
import React, { useState, useEffect } from 'react';
import { Descriptions, Card, Row, Col, App, Button, theme, Table, Input, Tabs } from 'antd';
import { createStyles } from 'antd-style';
import dayjs, { Dayjs } from 'dayjs';
import { CopyOutlined } from '@ant-design/icons';
import { calculateBazi as calculateBirthChart } from '../../algorithms/bazi';
import { BaziResult } from './types';
import { generateBaziAIDescription } from './baziPrompts';
import { useT } from '../../i18n/client';
import { copyToClipboard } from '../../utils/clipboard';

const useStyle = createStyles(({ token, css }) => ({
  container: css`
    background: ${token.colorBgContainer};
    border-radius: ${token.borderRadiusLG}px;
  `,
  aiCard: css`
    margin-top: 24px;
    background: ${token.colorFillAlter};

    .ant-card-head {
      @media (max-width: 576px) {
        min-height: auto;
        padding: 12px;

        .ant-card-head-wrapper {
          flex-wrap: wrap;
          gap: 8px;

          .ant-card-head-title {
            padding: 0;
            white-space: normal;
            display: -webkit-box;
            -webkit-line-clamp: 2;
            -webkit-box-orient: vertical;
          }

          .ant-card-extra {
            padding: 0;
            margin-left: auto;
          }
        }
      }
    }
  `,
  headerActions: css`
    @media (max-width: 576px) {
      .ant-btn {
        font-size: 12px;
        padding: 4px 8px;
        height: auto;

        .anticon {
          font-size: 12px;
        }
      }
    }
  `,
  aiTextArea: css`
    font-family: monospace;
    background: #f5f5f5;
    color: #333;
    border-radius: ${token.borderRadius}px;
  `,
  listItem: css`
    display: flex;
    align-items: center;
    padding: 12px 0;
    border-bottom: 1px solid ${token.colorBorderSecondary};
    &:last-child {
      border-bottom: none;
    }
  `,
  wuxingBar: css`
    flex: 1;
    height: 12px;
    background: #f0f0f0;
    border-radius: 6px;
    margin: 0 12px;
    overflow: hidden;
  `,
  quoteBox: css`
    padding: 12px 16px;
    background: ${token.colorFillQuaternary};
    border-left: 4px solid ${token.colorPrimary};
    margin-bottom: 16px;
    border-radius: 0 ${token.borderRadiusSM}px ${token.borderRadiusSM}px 0;
    color: ${token.colorTextSecondary};
    font-size: 13px;
    font-style: italic;
    line-height: 1.8;

    p {
      margin: 0;
      &:not(:last-child) {
        margin-bottom: 4px;
      }
    }
  `,
}));

interface BaziCalculatorProps {
  date: Dayjs;
  time: Dayjs;
  gender: 'male' | 'female';
  locationInfo?: {
    location?: string;
    longitude: number;
    latitude: number;
    useTrueSolarTime: boolean;
  };
}

export default function BaziCalculator({ date, time, gender, locationInfo }: BaziCalculatorProps) {
  const { styles } = useStyle();
  const { token } = theme.useToken();
  const { message } = App.useApp();
  const t = useT();
  const [baziResult, setBaziResult] = useState<BaziResult | null>(null);
  const [selectedDaYunIndex, setSelectedDaYunIndex] = useState<number>(0);

  useEffect(() => {
    const calculateBazi = () => {
      try {
        const { result, initialDaYunIndex } = calculateBirthChart(date, time, gender);
        setSelectedDaYunIndex(initialDaYunIndex);
        setBaziResult(result);
      } catch (error) {
        console.error('Bazi calculation error:', error);
        message.error('八字计算发生错误，请检查日期和时间是否有效');
      }
    };

    calculateBazi();
  }, [date, time, gender, message]);

  const handleCopy = () => {
    const text = generateBaziAIDescription(
      baziResult,
      locationInfo
        ? {
            ...locationInfo,
            trueSolarTimeStr: date.format('YYYY-MM-DD HH:mm:ss'),
          }
        : undefined
    );
    if (text) {
      copyToClipboard(
        text,
        () => {
          message.success('已复制完整排盘数据，可直接发送给AI助手');
        },
        () => {
          message.error('复制失败，请手动复制');
        }
      );
    }
  };

  if (!baziResult) return null;

  return (
    <div data-calendar-section="bazi" data-birth-date={date.format('YYYY-MM-DD')} data-birth-time={time.format('HH:mm:ss')} data-gender={gender} className={styles.container}>
      <Row gutter={[24, 24]}>
        <Col span={24}>
          <Card title="基本信息" variant="borderless">
            <Descriptions column={{ xs: 1, sm: 2, md: 3 }} style={{ marginBottom: 24 }}>
              <Descriptions.Item label="阳历">{baziResult.solarDate}</Descriptions.Item>
              <Descriptions.Item label="农历">{baziResult.lunarDate}</Descriptions.Item>
              <Descriptions.Item label="性别">{baziResult.gender}</Descriptions.Item>
              <Descriptions.Item label="年龄">
                {baziResult.age}岁 (虚岁{baziResult.chineseAge})
              </Descriptions.Item>
              {locationInfo && (
                <>
                  <Descriptions.Item label="地点">
                    {locationInfo.location || '未指定'}
                  </Descriptions.Item>
                  <Descriptions.Item label="经纬">
                    {locationInfo.longitude.toFixed(2)}°E, {locationInfo.latitude.toFixed(2)}°N
                  </Descriptions.Item>
                  <Descriptions.Item label="真太阳">
                    {locationInfo.useTrueSolarTime ? '已启用' : '未启用'}
                  </Descriptions.Item>
                </>
              )}
              <Descriptions.Item label="胎元">{baziResult.taiYuan}</Descriptions.Item>
              <Descriptions.Item label="命宫">{baziResult.mingGong}</Descriptions.Item>
              <Descriptions.Item label="身宫">{baziResult.shenGong}</Descriptions.Item>
              <Descriptions.Item label="空亡">{baziResult.xunKong}</Descriptions.Item>
              <Descriptions.Item label="日主">{baziResult.dayMaster}</Descriptions.Item>
              <Descriptions.Item label="格局">{baziResult.geJu}</Descriptions.Item>
            </Descriptions>
            <Descriptions column={1}>
              <Descriptions.Item label="起运">{baziResult.qiyun}</Descriptions.Item>
              <Descriptions.Item label="黄道">
                {baziResult.huangDao.ecliptic} ({baziResult.huangDao.duty}日 /{' '}
                {baziResult.huangDao.twelveStar})
              </Descriptions.Item>
              <Descriptions.Item label="神煞">{baziResult.shenSha.join('、')}</Descriptions.Item>
            </Descriptions>
          </Card>
        </Col>

        <Col span={24}>
          <Card title="八字排盘" variant="borderless">
            <Row gutter={[16, 16]}>
              {[
                {
                  name: '年柱',
                  stem: baziResult.eightChar.getYear().getHeavenStem(),
                  branch: baziResult.eightChar.getYear().getEarthBranch(),
                  nayin: baziResult.eightChar.getYear().getSound().getName(),
                },
                {
                  name: '月柱',
                  stem: baziResult.eightChar.getMonth().getHeavenStem(),
                  branch: baziResult.eightChar.getMonth().getEarthBranch(),
                  nayin: baziResult.eightChar.getMonth().getSound().getName(),
                },
                {
                  name: '日柱',
                  stem: baziResult.eightChar.getDay().getHeavenStem(),
                  branch: baziResult.eightChar.getDay().getEarthBranch(),
                  nayin: baziResult.eightChar.getDay().getSound().getName(),
                },
                {
                  name: '时柱',
                  stem: baziResult.eightChar.getHour().getHeavenStem(),
                  branch: baziResult.eightChar.getHour().getEarthBranch(),
                  nayin: baziResult.eightChar.getHour().getSound().getName(),
                },
              ].map((item) => (
                <Col key={item.name} xs={12} sm={6}>
                  <Card title={item.name} size="small" style={{ textAlign: 'center' }}>
                    <div style={{ fontSize: 24, fontWeight: 'bold', color: '#1677ff' }}>
                      {item.stem.getName()}
                    </div>
                    <div style={{ fontSize: 24, fontWeight: 'bold', color: '#1677ff' }}>
                      {item.branch.getName()}
                    </div>
                    <div style={{ fontSize: 12, color: '#999', marginTop: 8 }}>{item.nayin}</div>
                  </Card>
                </Col>
              ))}
            </Row>
          </Card>
        </Col>

        <Col span={24}>
          <Card title="五行统计" variant="borderless">
            <Row gutter={24}>
              <Col span={24} md={12}>
                <div>
                  {Object.entries(baziResult.wuxingCounts).map(([wx, count]) => (
                    <div key={wx} className={styles.listItem}>
                      <span style={{ width: 40, fontWeight: 'bold' }}>{wx}</span>
                      <div className={styles.wuxingBar}>
                        <div
                          style={{
                            width: `${Math.min((count / 8) * 100, 100)}%`,
                            height: '100%',
                            background: count > 3 ? '#ff4d4f' : count < 1 ? '#d9d9d9' : '#1677ff',
                          }}
                        />
                      </div>
                      <span style={{ width: 40, textAlign: 'right' }}>{count}</span>
                    </div>
                  ))}
                </div>
              </Col>
              <Col span={24} md={12}>
                <div style={{ maxHeight: 300, overflowY: 'auto' }}>
                  {Object.entries(baziResult.wuxingDetail).map(([wx, detail]) => (
                    <div key={wx} style={{ marginBottom: 12 }}>
                      <div style={{ fontWeight: 'bold', marginBottom: 4 }}>
                        {wx} (共{detail.count}个)
                      </div>
                      <div style={{ fontSize: 12, color: '#666' }}>{detail.sources.join('、')}</div>
                    </div>
                  ))}
                </div>
              </Col>
            </Row>
          </Card>
        </Col>

        <Col span={24}>
          <Card title="十神分析" variant="borderless">
            <Descriptions column={{ xs: 1, sm: 2, md: 4 }}>
              {Object.entries(baziResult.tenGods).map(([gan, god]) => (
                <Descriptions.Item key={gan} label={gan}>
                  {god}{' '}
                  <span style={{ fontSize: 12, color: '#999' }}>
                    ({baziResult.tenGodsWuxing[gan]})
                  </span>
                </Descriptions.Item>
              ))}
            </Descriptions>
          </Card>
        </Col>

        <Col span={24}>
          <Card title="大运" variant="borderless">
            <Tabs
              activeKey={String(selectedDaYunIndex)}
              onChange={(key) => setSelectedDaYunIndex(Number(key))}
              type="card"
              items={baziResult.daYunDetails.map((yun, index) => ({
                key: String(index),
                label: (
                  <div style={{ textAlign: 'center', padding: '0 8px' }}>
                    <div>
                      {yun.name} <span style={{ fontSize: 12, color: '#999' }}>({yun.wuxing})</span>
                    </div>
                    <div style={{ fontSize: 12, color: '#999' }}>
                      {yun.startAge}-{yun.endAge}岁
                    </div>
                  </div>
                ),
                children: (
                  <Table
                    dataSource={yun.years}
                    rowKey="year"
                    pagination={false}
                    size="small"
                    expandable={{
                      expandedRowRender: (record) => (
                        <div style={{ padding: '8px 0' }}>
                          <div style={{ fontWeight: 'bold', marginBottom: 8 }}>
                            {record.year}年 ({record.ganZhi}) 流月运势：
                          </div>
                          <Row gutter={[16, 16]}>
                            {record.months.map((item, idx) => (
                              <Col key={idx} xs={12} sm={8} md={6} lg={4}>
                                <div
                                  style={{
                                    background: token.colorFillAlter,
                                    padding: 8,
                                    borderRadius: 4,
                                    textAlign: 'center',
                                  }}
                                >
                                  <div style={{ fontSize: 12, color: '#666' }}>{item.month}</div>
                                  <div style={{ fontWeight: 'bold', margin: '4px 0' }}>
                                    {item.ganZhi}{' '}
                                    <span
                                      style={{
                                        fontSize: 12,
                                        color: '#999',
                                        fontWeight: 'normal',
                                      }}
                                    >
                                      ({item.wuxing})
                                    </span>
                                  </div>
                                  <div style={{ fontSize: 12, color: '#999' }}>{item.tenGod}</div>
                                </div>
                              </Col>
                            ))}
                          </Row>
                        </div>
                      ),
                      rowExpandable: () => true,
                    }}
                    columns={[
                      {
                        title: '岁数',
                        dataIndex: 'age',
                        key: 'age',
                        width: 60,
                        render: (age) => `${age}岁`,
                      },
                      {
                        title: '流年',
                        key: 'year',
                        render: (_, record) => (
                          <div>
                            <span style={{ fontWeight: 'bold', marginRight: 8 }}>
                              {record.year}
                            </span>
                            <span style={{ color: token.colorPrimary, marginRight: 4 }}>
                              {record.ganZhi}
                            </span>
                            <span style={{ fontSize: 12, color: '#999', marginRight: 8 }}>
                              ({record.wuxing})
                            </span>
                            <span style={{ fontSize: 12, color: '#666' }}>{record.tenGod}</span>
                          </div>
                        ),
                      },
                      {
                        title: '小运',
                        key: 'xiaoYun',
                        render: (_, record) => (
                          <div>
                            <span style={{ fontWeight: 'bold', marginRight: 4 }}>
                              {record.xiaoYun}
                            </span>
                            <span style={{ fontSize: 12, color: '#999', marginRight: 8 }}>
                              ({record.xiaoYunWuXing})
                            </span>
                            <span style={{ fontSize: 12, color: '#666' }}>
                              {record.xiaoYunTenGod}
                            </span>
                          </div>
                        ),
                      },
                    ]}
                  />
                ),
              }))}
            />
          </Card>
        </Col>

        <Col span={24}>
          <Card
            title="AI解读助手(八字排盘)"
            className={styles.aiCard}
            extra={
              <div className={styles.headerActions}>
                <Button type="primary" icon={<CopyOutlined />} onClick={handleCopy}>
                  一键复制完整数据
                </Button>
              </div>
            }
          >
            <div className={styles.quoteBox}>
              <p>{t('divination.quote.part1')}</p>
              <p>{t('divination.quote.part2')}</p>
            </div>
            <div style={{ marginBottom: 16, color: '#666' }}>{t('divination.instruction')}</div>

            <Input.TextArea
              value={generateBaziAIDescription(
                baziResult,
                locationInfo
                  ? {
                      ...locationInfo,
                      trueSolarTimeStr: date.format('YYYY-MM-DD HH:mm:ss'),
                    }
                  : undefined
              )}
              autoSize={{ minRows: 4, maxRows: 10 }}
              readOnly
              className={styles.aiTextArea}
            />
            <div
              style={{
                marginBottom: 16,
                fontSize: 12,
                color: '#999',
                lineHeight: 1.5,
                whiteSpace: 'pre-wrap',
              }}
            >
              {t('divination.aiHelperTips')}
            </div>
          </Card>
        </Col>
      </Row>
    </div>
  );
}
