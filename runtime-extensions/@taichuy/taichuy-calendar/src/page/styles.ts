import { createStyles } from 'antd-style';

export const usePageStyles = createStyles(({ token, css }) => ({
  container: css`
    max-width: 1200px;
    margin: 0 auto;
    padding: 24px;
    @media (max-width: 768px) {
      padding: 16px;
      display: flex;
      flex-direction: column;
    }
  `,
  mobileCalendar: css`
    display: none;
    @media (max-width: 768px) {
      display: block;
      margin-bottom: 16px;
    }
  `,
  formCard: css`
    margin-bottom: 24px;
    background: ${token.colorBgContainer};
    border-radius: ${token.borderRadiusLG}px;
    box-shadow: 0 1px 2px 0 rgba(0, 0, 0, 0.05);
    .ant-card-body {
      padding: 24px;
    }
    @media (max-width: 768px) {
      margin-bottom: 16px;
      .ant-card-body {
        padding: 16px;
      }
    }
  `,
  // Custom style for form rows
  formRow: css`
    display: flex;
    gap: 24px;
    margin-bottom: 24px;
    align-items: center;

    &:last-child {
      margin-bottom: 0;
    }

    .ant-form-item {
      margin-bottom: 0;
      flex: 1;
    }

    @media (max-width: 768px) {
      flex-direction: column;
      align-items: stretch;
      gap: 16px;
      margin-bottom: 16px;

      .ant-form-item {
        width: 100%;
      }
    }
  `,
  tabCard: css`
    .ant-tabs-nav {
      margin-bottom: 16px;
    }
  `,
  dateCell: css`
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0px; /* Reduced gap */
  `,
  solarDate: css`
    font-size: 16px; /* Larger font for solar date */
    line-height: 1.2;
    font-weight: 600;
    font-family:
      -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
  `,
  lunarDate: css`
    font-size: 11px; /* Adjusted font size */
    line-height: 1;
    color: #888;
    transform: scale(0.9);
    white-space: nowrap;
    margin-top: 2px;
  `,
  // Popup styles to override Ant Design default cell size
  datePickerPopup: css`
    .ant-picker-content {
      table-layout: fixed;
      width: 100%;
    }

    .ant-picker-cell {
      padding: 4px 0; /* Add vertical spacing between rows */
    }

    .ant-picker-cell-inner {
      width: 46px !important; /* Wider cell */
      height: 46px !important; /* Taller cell */
      border-radius: 8px !important;
      display: flex !important;
      flex-direction: column !important;
      justify-content: center !important;
      align-items: center !important;

      &:before {
        border-radius: 8px !important; /* Match border radius for selected state */
      }
    }

    /* Ensure the header row aligns */
    .ant-picker-header {
      margin-bottom: 8px;
    }

    /* Mobile adjustments if necessary */
    @media (max-width: 768px) {
      .ant-picker-cell-inner {
        width: 40px !important;
        height: 40px !important;
      }
      .solar-date {
        font-size: 14px;
      }
      .lunar-date {
        font-size: 10px;
      }
    }
  `,
}));
