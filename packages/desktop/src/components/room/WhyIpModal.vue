<script setup lang="ts">
import ModalDialog from "../ui/ModalDialog.vue";

const open = defineModel<boolean>({ required: true });
</script>

<template>
  <ModalDialog v-model="open" title="为什么不是 192.168 开头？" :width="440">
    <p class="why-text">
      192.168 是你家路由器的地址段，只有同一台路由器下的设备能互通。LanLink 给你的是虚拟网卡的地址
      （10.126.126 开头），朋友的电脑上也有一张同样的网卡，所以跨城市也能像在一个局域网里。
    </p>
    <p class="why-text">
      游戏里把它当成一台普通电脑的 IP 就行：直接连接、填这个地址，再加上游戏自己的端口。
    </p>
  </ModalDialog>
</template>

<style scoped>
.why-text {
  margin: 0 0 10px;
  line-height: 1.7;
}

.why-text:last-child {
  margin-bottom: 0;
}
</style>
