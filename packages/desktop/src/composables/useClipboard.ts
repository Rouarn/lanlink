import { ref, type Ref } from "vue";
import { copyPlainText } from "../lib/clipboard";

export interface CopyFeedback {
  /** 是否刚复制成功，用于按钮文案切换 */
  copied: Ref<boolean>;
  copy: (text: string) => Promise<void>;
}

/** 复制并在短暂时间后复位提示状态 */
export function useCopyFeedback(resetDelay = 1600): CopyFeedback {
  const copied = ref(false);
  let timer: number | undefined;

  async function copy(text: string): Promise<void> {
    if (!text) return;
    await copyPlainText(text);
    copied.value = true;
    if (timer) window.clearTimeout(timer);
    timer = window.setTimeout(() => {
      copied.value = false;
    }, resetDelay);
  }

  return { copied, copy };
}
