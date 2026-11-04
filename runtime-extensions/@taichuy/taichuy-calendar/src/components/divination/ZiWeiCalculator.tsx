'use client';
import type { ChartAIHelperRenderer } from '../../../types';
import React, { useState, useEffect, useMemo } from 'react';
import { Button, Card, Input, App, Tooltip } from 'antd';
import { CopyOutlined, LeftOutlined, RightOutlined, FileTextOutlined } from '@ant-design/icons';
import { useZiWeiStyles as useStyle } from './ziwei-styles';
import { PALACE_GRID_POSITIONS } from './palace-layout';
import dayjs, { Dayjs } from 'dayjs';
import { calculateZiWei } from '../../algorithms/ziwei';
import type { IFunctionalAstrolabe } from 'iztro/lib/astro/FunctionalAstrolabe';
import type { IFunctionalPalace } from 'iztro/lib/astro/FunctionalPalace';
import type { Decadal } from 'iztro/lib/data/types';
import ZiWeiMobile from './ZiWeiMobile';
import {
  generateZiWeiAIDescription,
  generateZiWeiAIDescriptionWithHoroscope,
} from './ziweiPrompts';
import { useT } from '../../i18n/client';
import { copyToClipboard } from '../../utils/clipboard';

// Define the grid mapping for the 12 Earthly Branches (Standard Zi Wei Dou Shu Layout)
// x/y coordinates are percentages of the container for SVG lines (center of each cell)
interface ZiWeiCalculatorProps {
  renderAIHelper?: ChartAIHelperRenderer;
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

export default function ZiWeiCalculator({
  date,
  time,
  gender,
  locationInfo,
  renderAIHelper,
}: ZiWeiCalculatorProps) {
  const { styles } = useStyle();
  const { message } = App.useApp();
  const t = useT();
  const [astrolabe, setAstrolabe] = useState<IFunctionalAstrolabe | null>(null);
  const [viewDate, setViewDate] = useState<Dayjs>(dayjs());
  const [focusedPalaceIndex, setFocusedPalaceIndex] = useState<number | null>(null);

  // Calculate Age
  const age = dayjs().diff(date, 'year');
  const chineseAge = dayjs().year() - date.year() + 1;

  const horoscope = useMemo(() => {
    if (astrolabe && viewDate) {
      try {
        return astrolabe.horoscope(viewDate.toDate());
      } catch (e) {
        console.error('Horoscope error', e);
        return null;
      }
    }
    return null;
  }, [astrolabe, viewDate]);

  // Initial Calculation
  useEffect(() => {
    const calculate = async () => {
      try {
        const result = calculateZiWei(date, time, gender);

        setAstrolabe(result);

        // Find Ming Palace index using native method
        const mingPalace = result.palace('命宫');
        if (mingPalace) {
          setFocusedPalaceIndex(mingPalace.index);
        }
      } catch (error) {
        console.error('Calculation error:', error);
        message.error('排盘计算发生错误，请检查日期是否有效');
      }
    };

    calculate();
  }, [date, time, gender, message]);

  // Time Change Handler
  const handleTimeChange = (
    unit: 'limit' | 'year' | 'month' | 'day' | 'hour',
    direction: 1 | -1
  ) => {
    let newDate = viewDate.clone();
    switch (unit) {
      case 'limit':
        newDate = newDate.add(direction * 10, 'year');
        break;
      case 'year':
        newDate = newDate.add(direction, 'year');
        break;
      case 'month':
        newDate = newDate.add(direction, 'month');
        break;
      case 'day':
        newDate = newDate.add(direction, 'day');
        break;
      case 'hour':
        newDate = newDate.add(direction * 2, 'hour');
        break;
    }

    // Validation: Cannot go before birth time
    // Construct birth datetime from props (date has YMD, time has HM)
    const birthDateTime = date.clone().hour(time.hour()).minute(time.minute()).second(0);

    if (newDate.isBefore(birthDateTime)) {
      return;
    }

    setViewDate(newDate);
  };

  const checkCanGoBack = (unit: 'limit' | 'year' | 'month' | 'day' | 'hour') => {
    const birthDateTime = date.clone().hour(time.hour()).minute(time.minute()).second(0);
    let newDate = viewDate.clone();
    switch (unit) {
      case 'limit':
        newDate = newDate.add(-10, 'year');
        break;
      case 'year':
        newDate = newDate.add(-1, 'year');
        break;
      case 'month':
        newDate = newDate.add(-1, 'month');
        break;
      case 'day':
        newDate = newDate.add(-1, 'day');
        break;
      case 'hour':
        newDate = newDate.add(-2, 'hour');
        break;
    }
    return !newDate.isBefore(birthDateTime);
  };

  const resetToToday = () => {
    setViewDate(dayjs());
  };

  // Helper for Chinese Time
  const getChineseTime = (d: Dayjs) => {
    const h = d.hour();
    const branches = ['早子', '丑', '寅', '卯', '辰', '巳', '午', '未', '申', '酉', '戌', '亥'];
    if (h >= 23) return '晚子时';
    if (h < 1) return '早子时';
    const idx = Math.floor((h + 1) / 2) % 12;
    return branches[idx] + '时';
  };

  const getMutagenStyle = (mutagen: string) => {
    switch (mutagen) {
      case '禄':
        return styles.mutagenLu;
      case '权':
        return styles.mutagenQuan;
      case '科':
        return styles.mutagenKe;
      case '忌':
        return styles.mutagenJi;
      default:
        return styles.mutagen;
    }
  };

  // Generate SVG Lines
  const renderLines = () => {
    if (!astrolabe) return null;

    let targetIndex = focusedPalaceIndex;
    if (targetIndex === null) {
      targetIndex = astrolabe.palaces.findIndex((p) => p.name === '命宫');
    }

    if (targetIndex === null || targetIndex === -1) return null;

    const focusedPalace = astrolabe.palaces[targetIndex];
    if (!focusedPalace) return null;

    const startPos = PALACE_GRID_POSITIONS[focusedPalace.earthlyBranch];
    if (!startPos) return null;

    // Use iztro's surroundedPalaces to get the correct relationships
    const surrounded = astrolabe.surroundedPalaces(focusedPalace.index);

    // Helper to get position by earthly branch
    const getPos = (branch: string) => PALACE_GRID_POSITIONS[branch];

    const oppositePos = getPos(surrounded.opposite.earthlyBranch);
    const wealthPos = getPos(surrounded.wealth.earthlyBranch);
    const careerPos = getPos(surrounded.career.earthlyBranch);

    return (
      <svg className={styles.svgOverlay}>
        <defs>
          <marker id="dot" markerWidth="4" markerHeight="4" refX="2" refY="2">
            <circle cx="2" cy="2" r="2" fill="rgba(230, 63, 50, 0.8)" />
          </marker>
        </defs>
        {/* Lines to Trines (Wealth and Career) */}
        {wealthPos && (
          <line
            x1={`${startPos.x}%`}
            y1={`${startPos.y}%`}
            x2={`${wealthPos.x}%`}
            y2={`${wealthPos.y}%`}
            stroke="rgba(230, 63, 50, 0.1)"
            strokeWidth="1.5"
            strokeDasharray="5,3"
          />
        )}
        {careerPos && (
          <line
            x1={`${startPos.x}%`}
            y1={`${startPos.y}%`}
            x2={`${careerPos.x}%`}
            y2={`${careerPos.y}%`}
            stroke="rgba(230, 63, 50, 0.1)"
            strokeWidth="1.5"
            strokeDasharray="5,3"
          />
        )}
        {/* Line to Opposite - Make it solid and distinct */}
        {oppositePos && (
          <line
            x1={`${startPos.x}%`}
            y1={`${startPos.y}%`}
            x2={`${oppositePos.x}%`}
            y2={`${oppositePos.y}%`}
            stroke="rgba(230, 63, 50, 0.5)"
            strokeWidth="2"
          />
        )}
        {/* Connecting Trines to form triangle */}
        {wealthPos && careerPos && (
          <line
            x1={`${wealthPos.x}%`}
            y1={`${wealthPos.y}%`}
            x2={`${careerPos.x}%`}
            y2={`${careerPos.y}%`}
            stroke="rgba(230, 63, 50, 0.1)"
            strokeWidth="1.5"
            strokeDasharray="5,3"
          />
        )}
        {wealthPos && oppositePos && (
          <line
            x1={`${wealthPos.x}%`}
            y1={`${wealthPos.y}%`}
            x2={`${oppositePos.x}%`}
            y2={`${oppositePos.y}%`}
            stroke="rgba(230, 63, 50, 0.1)"
            strokeWidth="1"
            strokeDasharray="2"
          />
        )}
        {careerPos && oppositePos && (
          <line
            x1={`${careerPos.x}%`}
            y1={`${careerPos.y}%`}
            x2={`${oppositePos.x}%`}
            y2={`${oppositePos.y}%`}
            stroke="rgba(230, 63, 50, 0.1)"
            strokeWidth="1"
            strokeDasharray="2"
          />
        )}
      </svg>
    );
  };

  const renderPalace = (palace: IFunctionalPalace, index: number) => {
    const pos = PALACE_GRID_POSITIONS[palace.earthlyBranch];
    if (!pos) return null;

    const isFocused = focusedPalaceIndex === index;

    // Check if connected
    let connectionType: 'opposite' | 'trine' | null = null;
    if (astrolabe) {
      let targetIndex = focusedPalaceIndex;
      if (targetIndex === null) {
        targetIndex = astrolabe.palaces.findIndex((p) => p.name === '命宫');
      }
      if (targetIndex !== null && targetIndex !== -1) {
        const surrounded = astrolabe.surroundedPalaces(targetIndex);
        if (surrounded) {
          if (surrounded.opposite.index === index) {
            connectionType = 'opposite';
          } else if (surrounded.wealth.index === index || surrounded.career.index === index) {
            connectionType = 'trine';
          }
        }
      }
    }

    // Horoscope markers
    const isDecadal = horoscope?.decadal.index === index;
    const isYearly = horoscope?.yearly.index === index;
    const isMonthly = horoscope?.monthly.index === index;
    const isDaily = horoscope?.daily.index === index;

    // Dynamic Palace Names (e.g. from Yearly)
    const yearlyName = horoscope?.yearly?.palaceNames?.[index];

    return (
      <div
        key={palace.earthlyBranch}
        data-palace-index={index}
        data-palace-focused={isFocused ? 'true' : 'false'}
        className={`${styles.palaceCell} ${isFocused ? styles.palaceCellFocused : ''} ${
          connectionType === 'opposite'
            ? styles.palaceCellConnectedOpposite
            : connectionType === 'trine'
              ? styles.palaceCellConnectedTrine
              : ''
        }`}
        style={{
          gridRow: pos.gridRow,
          gridColumn: pos.gridColumn,
        }}
        onClick={() => setFocusedPalaceIndex(index)}
      >
        {/* Top: Stars */}
        <div className={styles.starsSection}>
          <div className={styles.majorStarsCol}>
            {palace.majorStars.map((star, idx) => (
              <div key={idx} className={styles.starMajor}>
                {star.name}
                <span className={styles.brightness}>{star.brightness}</span>
                {star.mutagen && (
                  <span className={`${styles.mutagen} ${getMutagenStyle(star.mutagen)}`}>
                    {star.mutagen}
                  </span>
                )}
              </div>
            ))}
          </div>
          <div className={styles.minorStarsCol}>
            {palace.minorStars.map((star, idx) => (
              <div key={`minor-${idx}`} className={styles.starMinor}>
                {star.name}
                {star.brightness && <span className={styles.brightness}>{star.brightness}</span>}
              </div>
            ))}
            {palace.adjectiveStars.map((star, idx) => (
              <div key={`adj-${idx}`} className={styles.starAdjective}>
                {star.name}
              </div>
            ))}
          </div>
        </div>

        {/* Bottom Section */}
        <div className={styles.bottomSection}>
          {/* Gods Row */}
          <div className={styles.godsRow}>
            <span>{palace.boshi12}</span>
            <span>{palace.jiangqian12}</span>
            <span>{palace.suiqian12}</span>
            <span>{palace.changsheng12}</span>
          </div>

          {/* Horoscope Markers Row */}
          {(isDecadal || isYearly || isMonthly || isDaily) && (
            <div style={{ display: 'flex', gap: 2, flexWrap: 'wrap', marginBottom: 2 }}>
              {isDecadal && (
                <span
                  style={{
                    background: '#722ed1',
                    color: '#fff',
                    fontSize: 9,
                    padding: '0 2px',
                    borderRadius: 2,
                  }}
                >
                  大限
                </span>
              )}
              {isYearly && (
                <span
                  style={{
                    background: '#eb2f96',
                    color: '#fff',
                    fontSize: 9,
                    padding: '0 2px',
                    borderRadius: 2,
                  }}
                >
                  流年
                </span>
              )}
              {isMonthly && (
                <span
                  style={{
                    background: '#fa8c16',
                    color: '#fff',
                    fontSize: 9,
                    padding: '0 2px',
                    borderRadius: 2,
                  }}
                >
                  流月
                </span>
              )}
              {isDaily && (
                <span
                  style={{
                    background: '#13c2c2',
                    color: '#fff',
                    fontSize: 9,
                    padding: '0 2px',
                    borderRadius: 2,
                  }}
                >
                  流日
                </span>
              )}
            </div>
          )}

          {/* Footer */}
          <div className={styles.footerSection}>
            <div className={styles.palaceNameContainer}>
              <span className={styles.palaceName}>{palace.name}</span>
              {yearlyName && yearlyName !== palace.name && (
                <span style={{ fontSize: 9, color: '#999', marginTop: 1 }}>{yearlyName}</span>
              )}
              {(palace.isBodyPalace || palace.isOriginalPalace) && (
                <span className={styles.palaceType}>
                  {palace.isBodyPalace ? '身宫' : ''}
                  {palace.isBodyPalace && palace.isOriginalPalace ? '/' : ''}
                  {palace.isOriginalPalace ? '来因' : ''}
                </span>
              )}
            </div>

            <div className={styles.stemBranchContainer}>
              <div>
                <span className={styles.heavenlyStem}>{palace.heavenlyStem}</span>
                <span className={styles.earthlyBranch}>{palace.earthlyBranch}</span>
              </div>
              <span className={styles.decadalRange}>
                {palace.decadal.range[0]} - {palace.decadal.range[1]}
              </span>
            </div>
          </div>

          {/* Ages */}
          <div className={styles.agesList}>{palace.ages.join(' ')}</div>
        </div>
      </div>
    );
  };

  const handleCopy = () => {
    const text = natalText;
    if (text) {
      copyToClipboard(
        text,
        () => message.success('已复制本命排盘数据'),
        () => message.error('复制失败')
      );
    }
  };

  const handleCopyHoroscope = () => {
    const text = horoscopeText;
    if (text) {
      copyToClipboard(
        text,
        () => message.success('已复制包含运限的排盘数据'),
        () => message.error('复制失败')
      );
    }
  };

  if (!astrolabe) return null;

  const natalText = generateZiWeiAIDescription(
    astrolabe,
    locationInfo ? { ...locationInfo, trueSolarTimeStr: date.format('YYYY-MM-DD HH:mm:ss') } : undefined,
    { age, chineseAge }, gender
  );
  const horoscopeText = generateZiWeiAIDescriptionWithHoroscope(
    astrolabe, horoscope,
    locationInfo ? { ...locationInfo, trueSolarTimeStr: date.format('YYYY-MM-DD HH:mm:ss') } : undefined,
    { age, chineseAge }, gender
  );

  return (
    <div data-calendar-section="ziwei" data-view-date={viewDate.format('YYYY-MM-DD HH:mm')} data-birth-date={date.format('YYYY-MM-DD')} data-birth-time={time.format('HH:mm:ss')} data-gender={gender} className={styles.container}>
      <div className={styles.mobileOnly}>
        <ZiWeiMobile
          astrolabe={astrolabe}
          horoscope={horoscope}
          viewDate={viewDate}
          gender={gender}
          locationInfo={locationInfo}
          onTimeChange={handleTimeChange}
          onResetDate={resetToToday}
          checkCanGoBack={checkCanGoBack}
        />
      </div>

      <div className={styles.desktopOnly}>
        <div className={styles.chartGrid}>
          {renderLines()}
          {astrolabe.palaces.map((palace, index) => renderPalace(palace, index))}

          {/* Center Info Cell */}
          <div className={styles.centerCell}>
            <div className={styles.centerInner}>
              <div
                className={styles.centerHeader}
                onClick={() => setFocusedPalaceIndex(null)}
                style={{ cursor: 'pointer' }}
              >
                <h3 className={styles.centerTitle}>紫微斗数</h3>
                <div style={{ fontSize: 12, color: '#999' }}>
                  {gender === 'male' ? '乾造' : '坤造'}
                </div>
              </div>

              {/* Basic Info */}
              <div className={styles.sectionTitle}>基本信息</div>
              <div className={styles.centerGrid}>
                {locationInfo && (
                  <>
                    <div className={styles.infoItem}>
                      <span className={styles.infoLabel}>地点</span>
                      <span className={styles.infoValue}>{locationInfo.location || '未指定'}</span>
                    </div>
                    <div className={styles.infoItem}>
                      <span className={styles.infoLabel}>经纬</span>
                      <span className={styles.infoValue} style={{ fontSize: 10 }}>
                        {locationInfo.longitude.toFixed(2)}, {locationInfo.latitude.toFixed(2)}
                      </span>
                    </div>
                    {locationInfo.useTrueSolarTime && (
                      <div className={styles.infoItem}>
                        <span className={styles.infoLabel}>真太阳</span>
                        <span className={styles.infoValue} style={{ fontSize: 10 }}>
                          {date.format('HH:mm:ss')}
                        </span>
                      </div>
                    )}
                  </>
                )}
                <div className={styles.infoItem}>
                  <span className={styles.infoLabel}>五行局</span>
                  <span className={`${styles.infoValue} ${styles.valueHighlight}`}>
                    {astrolabe.fiveElementsClass}
                  </span>
                </div>
                <div className={styles.infoItem}>
                  <span className={styles.infoLabel}>年龄</span>
                  <span className={styles.infoValue}>
                    {horoscope?.age?.nominalAge ? `${horoscope.age.nominalAge} 岁` : '-'}
                  </span>
                </div>

                <div className={styles.infoItem}>
                  <span className={styles.infoLabel}>四柱</span>
                  <span className={styles.infoValue}>{astrolabe.chineseDate}</span>
                </div>

                <div className={styles.infoItem}>
                  <span className={styles.infoLabel}>阳历</span>
                  <span className={styles.infoValue} style={{ fontSize: 11 }}>
                    {astrolabe.solarDate}
                  </span>
                </div>
                <div className={styles.infoItem}>
                  <span className={styles.infoLabel}>农历</span>
                  <span className={styles.infoValue} style={{ fontSize: 11 }}>
                    {astrolabe.lunarDate}
                  </span>
                </div>

                <div className={styles.infoItem}>
                  <span className={styles.infoLabel}>时辰</span>
                  <span className={styles.infoValue}>
                    {astrolabe.time} ({astrolabe.timeRange})
                  </span>
                </div>
                <div className={styles.infoItem}>
                  <span className={styles.infoLabel}>生肖</span>
                  <span className={styles.infoValue}>{astrolabe.zodiac}</span>
                </div>

                <div className={styles.infoItem}>
                  <span className={styles.infoLabel}>星座</span>
                  <span className={styles.infoValue}>{astrolabe.sign}</span>
                </div>
                <div className={styles.infoItem}>
                  <span className={styles.infoLabel}>命主</span>
                  <span className={styles.infoValue}>{astrolabe.soul}</span>
                </div>

                <div className={styles.infoItem}>
                  <span className={styles.infoLabel}>身主</span>
                  <span className={styles.infoValue}>{astrolabe.body}</span>
                </div>
                <div className={styles.infoItem}>
                  <span className={styles.infoLabel}>命宫</span>
                  <span className={styles.infoValue}>{astrolabe.earthlyBranchOfSoulPalace}</span>
                </div>
                <div className={styles.infoItem}>
                  <span className={styles.infoLabel}>身宫</span>
                  <span className={styles.infoValue}>{astrolabe.earthlyBranchOfBodyPalace}</span>
                </div>
              </div>

              {/* Horoscope Section */}
              <div className={styles.sectionTitle}>运限信息</div>

              <div
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 8,
                  width: '100%',
                  marginBottom: 8,
                }}
              >
                {/* Current Date Display */}
                <div
                  style={{
                    display: 'flex',
                    justifyContent: 'space-between',
                    alignItems: 'center',
                    fontSize: 12,
                  }}
                >
                  <div>
                    <div style={{ color: '#666' }}>农历：{horoscope?.lunarDate}</div>
                    <div style={{ color: '#666' }}>
                      阳历：{viewDate.format('YYYY-MM-DD')} {getChineseTime(viewDate)}
                    </div>
                  </div>
                  <Button size="small" onClick={resetToToday}>
                    今
                  </Button>
                </div>
              </div>

              <div className={styles.centerGrid}>
                {horoscope && (
                  <>
                    <div className={styles.infoItem}>
                      <span className={styles.infoLabel}>大限</span>
                      <span className={styles.infoValue}>
                        {horoscope.decadal.heavenlyStem}
                        {horoscope.decadal.earthlyBranch}
                        {(horoscope.decadal as unknown as Decadal).range && (
                          <span style={{ color: '#999', marginLeft: 4 }}>
                            ({(horoscope.decadal as unknown as Decadal).range[0]}-
                            {(horoscope.decadal as unknown as Decadal).range[1]})
                          </span>
                        )}
                      </span>
                    </div>
                    <div className={styles.infoItem}>
                      <span className={styles.infoLabel}>流年</span>
                      <span className={styles.infoValue}>
                        {horoscope.yearly.heavenlyStem}
                        {horoscope.yearly.earthlyBranch}
                      </span>
                    </div>
                    <div className={styles.infoItem}>
                      <span className={styles.infoLabel}>流月</span>
                      <span className={styles.infoValue}>
                        {horoscope.monthly.heavenlyStem}
                        {horoscope.monthly.earthlyBranch}
                      </span>
                    </div>
                    <div className={styles.infoItem}>
                      <span className={styles.infoLabel}>流日</span>
                      <span className={styles.infoValue}>
                        {horoscope.daily.heavenlyStem}
                        {horoscope.daily.earthlyBranch}
                      </span>
                    </div>
                  </>
                )}
              </div>

              {/* Navigation Control */}
              <div className={styles.navControlContainer}>
                <div className={styles.navGroup} style={{ justifyContent: 'flex-end' }}>
                  <Button
                    type="text"
                    size="small"
                    icon={<LeftOutlined />}
                    aria-label="limit-previous" onClick={() => handleTimeChange('limit', -1)}
                    disabled={!checkCanGoBack('limit')}
                    className={styles.navButton}
                  >
                    限
                  </Button>
                  <Button
                    type="text"
                    size="small"
                    icon={<LeftOutlined />}
                    aria-label="year-previous" onClick={() => handleTimeChange('year', -1)}
                    disabled={!checkCanGoBack('year')}
                    className={styles.navButton}
                  >
                    年
                  </Button>
                  <Button
                    type="text"
                    size="small"
                    icon={<LeftOutlined />}
                    aria-label="month-previous" onClick={() => handleTimeChange('month', -1)}
                    disabled={!checkCanGoBack('month')}
                    className={styles.navButton}
                  >
                    月
                  </Button>
                  <Button
                    type="text"
                    size="small"
                    icon={<LeftOutlined />}
                    aria-label="day-previous" onClick={() => handleTimeChange('day', -1)}
                    disabled={!checkCanGoBack('day')}
                    className={styles.navButton}
                  >
                    日
                  </Button>
                  <Button
                    type="text"
                    size="small"
                    icon={<LeftOutlined />}
                    aria-label="hour-previous" onClick={() => handleTimeChange('hour', -1)}
                    disabled={!checkCanGoBack('hour')}
                    className={styles.navButton}
                  >
                    时
                  </Button>
                </div>

                <div className={styles.timeDisplay}>{getChineseTime(viewDate)}</div>

                <div className={styles.navGroup} style={{ justifyContent: 'flex-start' }}>
                  <Button
                    type="text"
                    size="small"
                    aria-label="hour-next" onClick={() => handleTimeChange('hour', 1)}
                    className={styles.navButton}
                  >
                    时<RightOutlined />
                  </Button>
                  <Button
                    type="text"
                    size="small"
                    aria-label="day-next" onClick={() => handleTimeChange('day', 1)}
                    className={styles.navButton}
                  >
                    日<RightOutlined />
                  </Button>
                  <Button
                    type="text"
                    size="small"
                    aria-label="month-next" onClick={() => handleTimeChange('month', 1)}
                    className={styles.navButton}
                  >
                    月<RightOutlined />
                  </Button>
                  <Button
                    type="text"
                    size="small"
                    aria-label="year-next" onClick={() => handleTimeChange('year', 1)}
                    className={styles.navButton}
                  >
                    年<RightOutlined />
                  </Button>
                  <Button
                    type="text"
                    size="small"
                    aria-label="limit-next" onClick={() => handleTimeChange('limit', 1)}
                    className={styles.navButton}
                  >
                    限<RightOutlined />
                  </Button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      {renderAIHelper ? renderAIHelper({
        kind: 'ziwei', natalText,
        horoscopeText,
        viewDate: viewDate.format('YYYY-MM-DD HH:mm:ss'),
        copyNatal: handleCopy, copyHoroscope: handleCopyHoroscope,
      }) : (
        <Card
          title="AI 解读助手 (紫微斗数)"
          className={styles.aiCard}
          extra={
            <div className={styles.headerActions}>
              <Tooltip title="复制本命盘的基础数据，适用于分析先天命格">
                <Button icon={<CopyOutlined />} onClick={handleCopy}>
                  复制本命数据
                </Button>
              </Tooltip>
              <Tooltip title="复制本命盘及当前大限、流年等运限数据，适用于分析近期运势">
                <Button type="primary" icon={<FileTextOutlined />} onClick={handleCopyHoroscope}>
                  复制运限数据
                </Button>
              </Tooltip>
            </div>
          }
        >
          <div className={styles.quoteBox}>
            <p>{t('divination.quote.part1')}</p>
            <p>{t('divination.quote.part2')}</p>
          </div>
          <div style={{ marginBottom: 16, color: '#666' }}>{t('divination.instruction')}</div>

          <Input.TextArea
            value={natalText}
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
      )}
    </div>
  );
}
