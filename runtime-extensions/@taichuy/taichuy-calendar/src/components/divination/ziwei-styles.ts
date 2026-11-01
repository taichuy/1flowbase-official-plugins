import { createStyles } from 'antd-style';

export const useZiWeiStyles = createStyles(({ token, css, prefixCls }) => ({
  container: css`
    width: 100%;
    font-family: 'PingFang SC', 'Microsoft YaHei', sans-serif;
    background: ${token.colorBgContainer};
  `,
  mobileOnly: css`
    display: none;
    @media (max-width: 768px) {
      display: block;
    }
  `,
  desktopOnly: css`
    display: block;
    @media (max-width: 768px) {
      display: none;
    }
  `,
  chartGrid: css`
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    grid-template-rows: repeat(4, minmax(160px, auto));
    gap: 1px;
    margin-top: 24px;
    border: 1px solid ${token.colorBorderSecondary};
    background: ${token.colorBorderSecondary}; // Grid lines color
    padding: 1px;
    border-radius: 4px;
    overflow: hidden;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.05);
    position: relative; // For SVG overlay
  `,
  svgOverlay: css`
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 30; // Ensure it's above center cell (20)
  `,
  palaceCell: css`
    background: ${token.colorBgContainer};
    padding: 6px;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    min-height: 160px;
    position: relative;
    font-size: 12px;
    cursor: pointer;
    transition: background 0.2s;
    z-index: 5; // Below SVG

    &:hover {
      background: ${token.colorBgTextHover};
    }
  `,
  palaceCellFocused: css`
    background: ${token.colorFillQuaternary} !important;
    box-shadow: inset 0 0 0 2px ${token.colorPrimary}40;
  `,
  palaceCellConnectedOpposite: css`
    background: rgba(82, 196, 26, 0.2) !important;
    box-shadow: inset 0 0 0 2px rgba(82, 196, 26, 0.08);
  `,
  palaceCellConnectedTrine: css`
    background: rgba(82, 196, 26, 0.12) !important;
    box-shadow: inset 0 0 0 1px rgba(82, 196, 26, 0.15);
  `,
  navControlContainer: css`
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 4px;
    width: 100%;
    margin-top: 8px;
    background: #f5f5f5;
    border-radius: 8px;
    padding: 4px;
    container-type: inline-size;
  `,
  navGroup: css`
    display: flex;
    gap: 0;
    flex: 1;
    min-width: 0;
    overflow: hidden;
  `,
  navButton: css`
    padding: 0 4px !important;
    min-width: 24px !important;
    flex-shrink: 1;
    color: #666 !important;

    .anticon {
      font-size: 10px;
    }

    @container (max-width: 380px) {
      min-width: 20px !important;
      padding: 0 2px !important;

      .anticon {
        display: none;
      }
    }
  `,
  timeDisplay: css`
    font-size: 12px;
    font-weight: bold;
    white-space: nowrap;
    min-width: 40px;
    text-align: center;
    flex-shrink: 0;
    margin: 0 4px;

    @container (max-width: 350px) {
      font-size: 10px;
      margin: 0 2px;
      transform: scale(0.9);
    }
  `,
  // Top section: Stars
  starsSection: css`
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    flex: 1;
    margin-bottom: 4px;
  `,
  majorStarsCol: css`
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
    align-items: flex-start;
  `,
  minorStarsCol: css`
    display: flex;
    flex-direction: column;
    gap: 2px;
    align-items: flex-end;
    flex: 1;
    text-align: right;
  `,
  starMajor: css`
    font-weight: bold;
    color: #e63f32; // Standard Red for major stars
    font-size: 15px;
    display: inline-flex;
    align-items: center;
    line-height: 1.2;
  `,
  starMinor: css`
    color: #1890ff; // Blue for minor
    font-size: 12px;
    line-height: 1.2;
    font-weight: 500;
  `,
  starAdjective: css`
    color: #666;
    font-size: 11px;
    line-height: 1.2;
    transform: scale(0.95);
    transform-origin: right center;
  `,
  brightness: css`
    font-size: 10px;
    color: #999;
    margin-left: 2px;
    font-weight: normal;
    transform: scale(0.9);
  `,
  mutagen: css`
    font-size: 10px;
    color: #fff;
    border-radius: 2px;
    padding: 0 3px;
    margin-left: 4px;
    line-height: 14px;
    display: inline-block;
    font-weight: normal;
  `,
  mutagenLu: css`
    background: #e63f32; // Red for Lu
  `,
  mutagenQuan: css`
    background: #1890ff; // Blue for Quan
  `,
  mutagenKe: css`
    background: #52c41a; // Green for Ke
  `,
  mutagenJi: css`
    background: #333; // Black for Ji
  `,

  // Bottom section
  bottomSection: css`
    display: flex;
    flex-direction: column;
    gap: 2px;
  `,

  // Gods Row
  godsRow: css`
    display: flex;
    font-size: 10px;
    color: ${token.colorTextSecondary};
    gap: 4px;
    margin-bottom: 2px;
    transform: scale(0.9);
    transform-origin: left bottom;
  `,

  // Footer: Name, Stem, Decadal
  footerSection: css`
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    border-top: 1px dashed ${token.colorBorderSecondary};
    padding-top: 4px;
    position: relative;
  `,

  // Palace Name (Bottom Center/Left)
  palaceNameContainer: css`
    display: flex;
    flex-direction: column;
    align-items: center;
  `,
  palaceName: css`
    color: #fff;
    background: ${token.colorPrimary};
    padding: 1px 6px;
    border-radius: 2px;
    font-weight: bold;
    font-size: 12px;
    line-height: 1.4;
  `,
  palaceType: css`
    font-size: 10px;
    color: ${token.colorPrimary};
    margin-top: 1px;
    font-weight: bold;
  `,

  // Stem/Branch & Decadal (Bottom Right)
  stemBranchContainer: css`
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    line-height: 1.2;
  `,
  heavenlyStem: css`
    color: ${token.colorTextSecondary};
    font-size: 11px;
    margin-right: 2px;
  `,
  earthlyBranch: css`
    color: ${token.colorText};
    font-weight: bold;
    font-size: 14px;
  `,
  decadalRange: css`
    font-size: 12px;
    color: ${token.colorText};
    font-weight: 500;
  `,

  // Ages (Small Limit) - Bottom center/flowing
  agesList: css`
    font-size: 10px;
    color: ${token.colorTextQuaternary};
    text-align: center;
    margin-top: 2px;
    letter-spacing: 1px;
  `,

  // Center Cell
  centerCell: css`
    grid-column: 2 / 4;
    grid-row: 2 / 4;
    background: ${token.colorBgContainer};
    display: flex;
    flex-direction: column;
    padding: 8px;
    position: relative;
    z-index: 20; // Above SVG
  `,
  centerInner: css`
    flex: 1;
    border: 1px double ${token.colorBorderSecondary};
    border-radius: 4px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-start;
    overflow-y: auto;
  `,
  centerHeader: css`
    width: 100%;
    text-align: center;
    margin-bottom: 12px;
    border-bottom: 1px solid ${token.colorBorderSecondary};
    padding-bottom: 8px;
  `,
  centerTitle: css`
    font-size: 20px;
    font-weight: bold;
    color: ${token.colorPrimary};
    font-family: 'KaiTi', 'STKaiti', serif;
    letter-spacing: 4px;
    margin: 0;
  `,
  sectionTitle: css`
    font-size: 12px;
    font-weight: bold;
    color: ${token.colorTextTertiary};
    margin: 8px 0 4px;
    width: 100%;
    display: flex;
    align-items: center;

    &::after {
      content: '';
      flex: 1;
      height: 1px;
      background: #f0f0f0;
      margin-left: 8px;
    }
  `,
  centerGrid: css`
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px 16px;
    width: 100%;
  `,
  infoItem: css`
    display: flex;
    align-items: baseline;
    font-size: 12px;
    border-bottom: 1px dashed #f5f5f5;
    padding-bottom: 2px;
  `,
  infoLabel: css`
    color: #999;
    margin-right: 6px;
    white-space: nowrap;
    text-align: justify;
    text-align-last: justify;
    min-width: 32px;
  `,
  infoValue: css`
    color: #333;
    font-weight: 500;
    flex: 1;
  `,
  // Special red text for important values
  valueHighlight: css`
    color: #e63f32;
  `,

  horoscopeControl: css`
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
    width: 100%;

    .${prefixCls}-picker {
      flex: 1;
    }
  `,

  // AI Card
  aiCard: css`
    margin-top: 24px;
    background: ${token.colorFillAlter};

    .${prefixCls}-card-head {
      @media (max-width: 576px) {
        min-height: auto;
        padding: 12px;

        .${prefixCls}-card-head-wrapper {
          flex-wrap: wrap;
          gap: 8px;

          .${prefixCls}-card-head-title {
            padding: 0;
            white-space: normal;
            display: -webkit-box;
            -webkit-line-clamp: 2;
            -webkit-box-orient: vertical;
          }

          .${prefixCls}-card-extra {
            padding: 0;
            margin-left: auto;
          }
        }
      }
    }
  `,
  headerActions: css`
    display: flex;
    gap: 8px;
    align-items: center;

    @media (max-width: 576px) {
      flex-direction: column;
      align-items: flex-end;
      gap: 6px;

      .${prefixCls}-btn {
        font-size: 12px;
        padding: 4px 8px;
        height: auto;
        display: flex;
        align-items: center;

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
