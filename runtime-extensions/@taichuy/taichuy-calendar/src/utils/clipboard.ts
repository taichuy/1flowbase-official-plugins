import copy from 'copy-to-clipboard';

/**
 * 复制文本到剪贴板
 * @param text 要复制的文本
 * @param onSuccess 复制成功后的回调
 * @param onError 复制失败后的回调
 */
export const copyToClipboard = (
  text: string,
  onSuccess?: () => void,
  onError?: (error: Error) => void
) => {
  try {
    const result = copy(text, {
      debug: false,
      message: 'Press #{key} to copy',
    });
    if (result) {
      onSuccess?.();
    } else {
      onError?.(new Error('Copy failed'));
    }
  } catch (error) {
    onError?.(error instanceof Error ? error : new Error(String(error)));
  }
};
