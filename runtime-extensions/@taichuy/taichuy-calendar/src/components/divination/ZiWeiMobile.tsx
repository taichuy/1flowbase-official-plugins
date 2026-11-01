import React, { useState, useMemo } from 'react';
import { Tag, Button } from 'antd';
import { createStyles } from 'antd-style';
import { LeftOutlined, RightOutlined, ReloadOutlined } from '@ant-design/icons';
import { Dayjs } from 'dayjs';
import type { IFunctionalAstrolabe } from 'iztro/lib/astro/FunctionalAstrolabe';
import { PALACE_GRID_POSITIONS } from './palace-layout';

const useStyle = createStyles(({ token, css }) => ({
  container: css`
    width: 100%;
    font-family: 'PingFang SC', 'Microsoft YaHei', sans-serif;
    background: #fdfdfd;
    padding-bottom: 24px;
    position: relative;
    user-select: none;
  `,
  chartGrid: css`
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    grid-template-rows: repeat(4, minmax(80px, auto));
    gap: 1px;
    background: #d9d9d9;
    padding: 1px;
    border-radius: 4px;
    overflow: hidden;
    position: relative;
  `,
  svgOverlay: css`
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 30;
  `,
  palaceCell: css`
    background: #fff;
    padding: 2px;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    position: relative;
    font-size: 10px;
    cursor: pointer;
    transition: all 0.2s;
    z-index: 5;
    overflow: hidden;

    &:active {
      background: #f0f0f0;
    }
  `,
  palaceCellFocused: css`
    background: #fffbf0 !important;
    box-shadow: inset 0 0 0 2px ${token.colorPrimary};
  `,
  palaceCellConnectedOpposite: css`
    background: rgba(82, 196, 26, 0.2) !important;
  `,
  palaceCellConnectedTrine: css`
    background: rgba(82, 196, 26, 0.12) !important;
  `,
  centerCell: css`
    grid-column: 2 / 4;
    grid-row: 2 / 4;
    background: #fff;
    display: flex;
    flex-direction: column;
    padding: 4px;
    z-index: 20;
    overflow-y: auto;
    font-size: 10px;

    /* Hide scrollbar for cleaner look */
    &::-webkit-scrollbar {
      width: 0px;
      background: transparent;
    }
  `,
  miniStar: css`
    font-size: 9px;
    line-height: 1.1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  `,
  miniStarMajor: css`
    color: #e63f32;
    font-weight: bold;
  `,
  miniName: css`
    background: ${token.colorPrimary};
    color: #fff;
    font-size: 9px;
    padding: 0 2px;
    border-radius: 2px;
    align-self: center;
    margin-bottom: 1px;
    transform: scale(0.9);
  `,
  miniBranch: css`
    font-weight: bold;
    color: #ccc;
    position: absolute;
    bottom: 0;
    right: 1px;
    font-size: 10px;
  `,

  // Details Panel Styles
  detailsPanel: css`
    margin-top: 16px;
    padding: 0 12px;
  `,
  detailCard: css`
    background: #fff;
    border-radius: 8px;
    border: 1px solid #f0f0f0;
    padding: 12px;
    margin-bottom: 12px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.05);
  `,
  cardHeader: css`
    display: flex;
    justify-content: space-between;
    align-items: center;
    border-bottom: 1px solid #f0f0f0;
    padding-bottom: 8px;
    margin-bottom: 8px;
    font-weight: bold;
    font-size: 14px;
    color: ${token.colorText};
  `,
  cardTitle: css`
    display: flex;
    align-items: center;
    gap: 8px;
  `,
  mutagen: css`
    font-size: 10px;
    color: #fff;
    border-radius: 2px;
    padding: 0 3px;
    margin-left: 2px;
    display: inline-block;
  `,
  mutagenLu: css`
    background: #e63f32;
  `,
  mutagenQuan: css`
    background: #1890ff;
  `,
  mutagenKe: css`
    background: #52c41a;
  `,
  mutagenJi: css`
    background: #333;
  `,

  // Center Info Styles
  centerInfoRow: css`
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 2px;
    border-bottom: 1px dashed #f0f0f0;
    padding-bottom: 2px;
    &:last-child {
      border-bottom: none;
    }
  `,
  centerLabel: css`
    color: #999;
    transform: scale(0.9);
    transform-origin: left center;
  `,
  centerValue: css`
    color: #333;
    font-weight: 500;
  `,
  timeControl: css`
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: #f9f9f9;
    border-radius: 4px;
    padding: 2px 4px;
    margin-top: 2px;
  `,
  controlBtn: css`
    padding: 0 4px;
    height: 18px;
    font-size: 10px;
    display: flex;
    align-items: center;
  `,
}));

interface ZiWeiMobileProps {
  astrolabe: IFunctionalAstrolabe | null;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  horoscope: any;
  viewDate: Dayjs;
  gender: 'male' | 'female';
  locationInfo?: {
    location?: string;
    longitude: number;
    latitude: number;
    useTrueSolarTime: boolean;
  };
  onTimeChange: (unit: 'limit' | 'year' | 'month' | 'day' | 'hour', direction: 1 | -1) => void;
  onResetDate: () => void;
  checkCanGoBack: (unit: 'limit' | 'year' | 'month' | 'day' | 'hour') => boolean;
}

// Control Item Component
const ControlItem = ({
  label,
  value,
  unit,
  detail,
  disabledLeft = false,
  onTimeChange,
  styles,
}: {
  label: string;
  value: string;
  unit: 'limit' | 'year' | 'month' | 'day' | 'hour';
  detail?: string;
  disabledLeft?: boolean;
  onTimeChange: (unit: 'limit' | 'year' | 'month' | 'day' | 'hour', direction: 1 | -1) => void;
  styles: Record<string, string>;
}) => (
  <div className={styles.timeControl}>
    <span style={{ fontSize: 9, color: '#666', width: 24 }}>{label}</span>
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 4,
        flex: 1,
        justifyContent: 'space-between',
      }}
    >
      <Button
        type="text"
        size="small"
        icon={<LeftOutlined />}
        className={styles.controlBtn}
        aria-label={`${unit}-previous`}
        disabled={disabledLeft}
        style={{
          padding: '4px 8px',
          height: 24,
          width: 24,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
        }}
        onClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          if (!disabledLeft) onTimeChange(unit, -1);
        }}
      />
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', flex: 1 }}>
        <span style={{ fontSize: 11, fontWeight: 'bold' }}>{value}</span>
        {detail && <span style={{ fontSize: 9, color: '#999' }}>{detail}</span>}
      </div>
      <Button
        type="text"
        size="small"
        icon={<RightOutlined />}
        aria-label={`${unit}-next`}
        className={styles.controlBtn}
        style={{
          padding: '4px 8px',
          height: 24,
          width: 24,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
        }}
        onClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onTimeChange(unit, 1);
        }}
      />
    </div>
  </div>
);

export default function ZiWeiMobile({
  astrolabe,
  horoscope,
  viewDate,
  gender,
  locationInfo,
  onTimeChange,
  onResetDate,
  checkCanGoBack,
}: ZiWeiMobileProps) {
  const { styles } = useStyle();
  const [focusedIndex, setFocusedIndex] = useState<number | null>(null);

  const handleCellClick = (index: number) => {
    setFocusedIndex(index);
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
        return '';
    }
  };

  // Calculate relationships
  const relationships = useMemo(() => {
    if (!astrolabe || focusedIndex === null) return null;
    const surrounded = astrolabe.surroundedPalaces(focusedIndex);
    return {
      opposite: surrounded.opposite.index,
      wealth: surrounded.wealth.index,
      career: surrounded.career.index,
    };
  }, [focusedIndex, astrolabe]);

  if (!astrolabe) return null;

  const renderLines = () => {
    if (focusedIndex === null) return null;

    const startPos = PALACE_GRID_POSITIONS[astrolabe.palaces[focusedIndex].earthlyBranch];
    if (!startPos || !relationships) return null;

    const getPos = (idx: number) => PALACE_GRID_POSITIONS[astrolabe.palaces[idx].earthlyBranch];

    const oppositePos = getPos(relationships.opposite);
    const wealthPos = getPos(relationships.wealth);
    const careerPos = getPos(relationships.career);

    return (
      <svg className={styles.svgOverlay}>
        <defs>
          <marker id="dotMobile" markerWidth="4" markerHeight="4" refX="2" refY="2">
            <circle cx="2" cy="2" r="2" fill="rgba(230, 63, 50, 0.8)" />
          </marker>
        </defs>
        {[oppositePos, wealthPos, careerPos].map((pos, i) => (
          <line
            key={i}
            x1={`${startPos.x}%`}
            y1={`${startPos.y}%`}
            x2={`${pos.x}%`}
            y2={`${pos.y}%`}
            stroke="rgba(230, 63, 50, 0.4)"
            strokeWidth="1.5"
            strokeDasharray={i === 0 ? '' : '3,2'}
          />
        ))}
        {/* Connect trines */}
        <line
          x1={`${wealthPos.x}%`}
          y1={`${wealthPos.y}%`}
          x2={`${careerPos.x}%`}
          y2={`${careerPos.y}%`}
          stroke="rgba(230, 63, 50, 0.2)"
          strokeWidth="1"
          strokeDasharray="2,2"
        />
      </svg>
    );
  };

  const renderPalaceDetail = (palaceIndex: number, title: string, badge?: string) => {
    const palace = astrolabe.palaces[palaceIndex];
    return (
      <div className={styles.detailCard} key={palaceIndex}>
        <div className={styles.cardHeader}>
          <div className={styles.cardTitle}>
            {badge && <Tag color="blue">{badge}</Tag>}
            <span>{palace.name}</span>
            <span style={{ fontSize: 12, fontWeight: 'normal', color: '#999' }}>
              {palace.heavenlyStem}
              {palace.earthlyBranch}
            </span>
          </div>
          <div style={{ fontSize: 12, color: '#666' }}>
            {palace.decadal.range[0]}-{palace.decadal.range[1]}岁
          </div>
        </div>

        {/* Major Stars */}
        <div style={{ marginBottom: 8 }}>
          <div style={{ fontSize: 12, color: '#999', marginBottom: 4 }}>主星</div>
          {palace.majorStars.length > 0 ? (
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
              {palace.majorStars.map((star, idx) => (
                <div
                  key={idx}
                  style={{
                    fontWeight: 'bold',
                    color: '#e63f32',
                    display: 'flex',
                    alignItems: 'center',
                  }}
                >
                  {star.name}
                  <span style={{ fontSize: 10, color: '#999', margin: '0 2px' }}>
                    {star.brightness}
                  </span>
                  {star.mutagen && (
                    <span className={`${styles.mutagen} ${getMutagenStyle(star.mutagen)}`}>
                      {star.mutagen}
                    </span>
                  )}
                </div>
              ))}
            </div>
          ) : (
            <div style={{ color: '#ccc', fontSize: 12 }}>无主星 (借对宫)</div>
          )}
        </div>

        {/* Minor Stars */}
        <div style={{ marginBottom: 8 }}>
          <div style={{ fontSize: 12, color: '#999', marginBottom: 4 }}>辅星/杂曜</div>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
            {palace.minorStars.map((star, idx) => (
              <span key={idx} style={{ color: '#1890ff', fontSize: 12 }}>
                {star.name}
              </span>
            ))}
            {palace.adjectiveStars.map((star, idx) => (
              <span key={idx} style={{ color: '#666', fontSize: 11 }}>
                {star.name}
              </span>
            ))}
          </div>
        </div>

        {/* Gods */}
        <div
          style={{
            display: 'flex',
            gap: 8,
            fontSize: 11,
            color: '#666',
            background: '#fafafa',
            padding: 4,
            borderRadius: 4,
          }}
        >
          <span>{palace.boshi12}</span>
          <span>{palace.jiangqian12}</span>
          <span>{palace.suiqian12}</span>
          <span>{palace.changsheng12}</span>
        </div>
      </div>
    );
  };

  const getChineseTime = (d: Dayjs) => {
    const h = d.hour();
    const branches = ['早子', '丑', '寅', '卯', '辰', '巳', '午', '未', '申', '酉', '戌', '亥'];
    if (h >= 23) return '晚子时';
    if (h < 1) return '早子时';
    const idx = Math.floor((h + 1) / 2) % 12;
    return branches[idx] + '时';
  };

  return (
    <div data-calendar-section="ziwei-mobile" className={styles.container}>
      <div className={styles.chartGrid}>
        {renderLines()}
        {astrolabe.palaces.map((palace, index) => {
          const pos = PALACE_GRID_POSITIONS[palace.earthlyBranch];
          const isFocused = focusedIndex === index;
          const isOpposite = relationships?.opposite === index;
          const isTrine = relationships?.wealth === index || relationships?.career === index;

          const isDecadal = horoscope?.decadal.index === index;
          const isYearly = horoscope?.yearly.index === index;

          return (
            <div
              key={index}
              data-palace-index={index}
              data-palace-focused={isFocused ? 'true' : 'false'}
              className={`${styles.palaceCell}
                 ${isFocused ? styles.palaceCellFocused : ''}
                 ${isOpposite ? styles.palaceCellConnectedOpposite : ''}
                 ${isTrine ? styles.palaceCellConnectedTrine : ''}
               `}
              style={{ gridRow: pos.gridRow, gridColumn: pos.gridColumn }}
              onClick={() => handleCellClick(index)}
            >
              {/* Top: Name & Markers */}
              <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center' }}>
                <span className={styles.miniName}>{palace.name}</span>
                <div
                  style={{ display: 'flex', gap: 1, flexWrap: 'wrap', justifyContent: 'center' }}
                >
                  {isDecadal && (
                    <div
                      style={{ width: 6, height: 6, borderRadius: '50%', background: '#722ed1' }}
                    />
                  )}
                  {isYearly && (
                    <div
                      style={{ width: 6, height: 6, borderRadius: '50%', background: '#eb2f96' }}
                    />
                  )}
                </div>
              </div>

              {/* Center: Major Stars */}
              <div
                style={{
                  flex: 1,
                  display: 'flex',
                  flexDirection: 'column',
                  alignItems: 'center',
                  justifyContent: 'center',
                  gap: 0,
                }}
              >
                {palace.majorStars.map((s, i) => (
                  <div key={i} className={`${styles.miniStar} ${styles.miniStarMajor}`}>
                    {s.name[0]}
                    {s.mutagen && (
                      <span
                        style={{
                          fontSize: 8,
                          color: '#fff',
                          background: s.mutagen === '忌' ? '#333' : '#e63f32',
                          borderRadius: 2,
                          marginLeft: 1,
                          padding: '0 1px',
                        }}
                      >
                        {s.mutagen}
                      </span>
                    )}
                  </div>
                ))}
              </div>

              {/* Bottom: Branch */}
              <div className={styles.miniBranch}>{palace.earthlyBranch}</div>
            </div>
          );
        })}

        {/* Center Cell - Minimal */}
        <div className={styles.centerCell} onClick={(e) => e.stopPropagation()}>
          <div
            style={{
              textAlign: 'center',
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              height: '100%',
            }}
          >
            <span
              style={{ fontWeight: 'bold', color: '#d48806', fontSize: 16, fontFamily: 'KaiTi' }}
            >
              紫微斗数
            </span>
            <span style={{ fontSize: 10, color: '#999', marginTop: 4 }}>
              {gender === 'male' ? '乾造' : '坤造'}
            </span>
            <span style={{ fontSize: 10, color: '#ccc', marginTop: 8 }}>(基本信息见下方)</span>
          </div>
        </div>
      </div>

      {/* Fixed Center Info Card */}
      <div
        className={styles.detailCard}
        style={{ marginTop: 12, marginBottom: 12, borderLeft: '3px solid #d48806' }}
      >
        <div className={styles.cardHeader} style={{ marginBottom: 8, paddingBottom: 8 }}>
          <div className={styles.cardTitle}>
            <span style={{ color: '#d48806' }}>基本信息 & 运限</span>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <ReloadOutlined style={{ fontSize: 14, color: '#1890ff' }} aria-label="horoscope-reset" onClick={onResetDate} />
          </div>
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, marginBottom: 12 }}>
          {locationInfo && (
            <>
              <div className={styles.centerInfoRow} style={{ gridColumn: '1 / -1' }}>
                <span className={styles.centerLabel}>地点</span>
                <span className={styles.centerValue}>{locationInfo.location || '未指定'}</span>
              </div>
              <div className={styles.centerInfoRow}>
                <span className={styles.centerLabel}>经纬</span>
                <span className={styles.centerValue} style={{ fontSize: 10 }}>
                  {locationInfo.longitude.toFixed(2)}, {locationInfo.latitude.toFixed(2)}
                </span>
              </div>
              <div className={styles.centerInfoRow}>
                <span className={styles.centerLabel}>真太阳</span>
                <span className={styles.centerValue}>
                  {locationInfo.useTrueSolarTime ? '开启' : '关闭'}
                </span>
              </div>
            </>
          )}
          <div className={styles.centerInfoRow}>
            <span className={styles.centerLabel}>五行局</span>
            <span className={styles.centerValue}>{astrolabe.fiveElementsClass}</span>
          </div>
          <div className={styles.centerInfoRow}>
            <span className={styles.centerLabel}>年龄</span>
            <span className={styles.centerValue}>
              {horoscope?.age?.nominalAge ? `${horoscope.age.nominalAge} 岁` : '-'}
            </span>
          </div>
          <div className={styles.centerInfoRow}>
            <span className={styles.centerLabel}>命/身主</span>
            <span className={styles.centerValue}>
              {astrolabe.soul} / {astrolabe.body}
            </span>
          </div>
          <div className={styles.centerInfoRow}>
            <span className={styles.centerLabel}>命/身宫</span>
            <span className={styles.centerValue}>
              {astrolabe.earthlyBranchOfSoulPalace} / {astrolabe.earthlyBranchOfBodyPalace}
            </span>
          </div>
          <div className={styles.centerInfoRow}>
            <span className={styles.centerLabel}>生肖</span>
            <span className={styles.centerValue}>{astrolabe.zodiac}</span>
          </div>
          <div className={styles.centerInfoRow}>
            <span className={styles.centerLabel}>星座</span>
            <span className={styles.centerValue}>{astrolabe.sign}</span>
          </div>
          <div className={styles.centerInfoRow} style={{ gridColumn: '1 / -1' }}>
            <span className={styles.centerLabel}>四柱</span>
            <span className={styles.centerValue}>{astrolabe.chineseDate}</span>
          </div>
          <div className={styles.centerInfoRow} style={{ gridColumn: '1 / -1' }}>
            <span className={styles.centerLabel}>阳历</span>
            <span className={styles.centerValue}>{astrolabe.solarDate}</span>
          </div>
          <div className={styles.centerInfoRow} style={{ gridColumn: '1 / -1' }}>
            <span className={styles.centerLabel}>农历</span>
            <span className={styles.centerValue}>{astrolabe.lunarDate}</span>
          </div>
        </div>

        {/* Horoscope Controls */}
        <div style={{ background: '#f9f9f9', padding: 8, borderRadius: 6 }}>
          <div
            style={{
              fontSize: 11,
              color: '#666',
              marginBottom: 6,
              borderBottom: '1px solid #eee',
              paddingBottom: 4,
            }}
          >
            <div>农历：{horoscope?.lunarDate}</div>
            <div>阳历：{viewDate.format('YYYY-MM-DD')}</div>
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            <ControlItem
              label="大限"
              value={`${horoscope?.decadal?.heavenlyStem || ''}${horoscope?.decadal?.earthlyBranch || ''}`}
              detail={
                horoscope?.decadal?.range
                  ? `${horoscope.decadal.range[0]}-${horoscope.decadal.range[1]}岁`
                  : ''
              }
              unit="limit"
              disabledLeft={!checkCanGoBack('limit')}
              onTimeChange={onTimeChange}
              styles={styles}
            />
            <ControlItem
              label="流年"
              value={`${horoscope?.yearly?.heavenlyStem || ''}${horoscope?.yearly?.earthlyBranch || ''}`}
              unit="year"
              disabledLeft={!checkCanGoBack('year')}
              onTimeChange={onTimeChange}
              styles={styles}
            />
            <ControlItem
              label="流月"
              value={`${horoscope?.monthly?.heavenlyStem || ''}${horoscope?.monthly?.earthlyBranch || ''}`}
              unit="month"
              disabledLeft={!checkCanGoBack('month')}
              onTimeChange={onTimeChange}
              styles={styles}
            />
            <ControlItem
              label="流日"
              value={`${horoscope?.daily?.heavenlyStem || ''}${horoscope?.daily?.earthlyBranch || ''}`}
              unit="day"
              disabledLeft={!checkCanGoBack('day')}
              onTimeChange={onTimeChange}
              styles={styles}
            />
            <ControlItem
              label="流时"
              value={getChineseTime(viewDate)}
              detail={viewDate.format('HH:mm')}
              unit="hour"
              disabledLeft={!checkCanGoBack('hour')}
              onTimeChange={onTimeChange}
              styles={styles}
            />
          </div>
        </div>
      </div>

      {/* Details Panel Below Grid */}
      {focusedIndex !== null && relationships && (
        <div className={styles.detailsPanel}>
          {/* 1. Focused Palace */}
          {renderPalaceDetail(focusedIndex, '当前宫位', '本宫')}

          {/* 2. Opposite Palace */}
          {renderPalaceDetail(relationships.opposite, '对宫', '对宫')}

          {/* 3. Trine Palaces */}
          {renderPalaceDetail(relationships.wealth, '三合', '三方')}
          {renderPalaceDetail(relationships.career, '三合', '三方')}
        </div>
      )}

      {focusedIndex === null && (
        <div style={{ textAlign: 'center', padding: 24, color: '#999', fontSize: 12 }}>
          点击宫位查看详细星曜与运势
        </div>
      )}
    </div>
  );
}
