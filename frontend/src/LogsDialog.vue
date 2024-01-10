/*-
* ========================LICENSE_START=================================
* PREvant Frontend
* %%
* Copyright (C) 2018 - 2019 aixigo AG
* %%
* Permission is hereby granted, free of charge, to any person obtaining a copy
* of this software and associated documentation files (the "Software"), to deal
* in the Software without restriction, including without limitation the rights
* to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
* copies of the Software, and to permit persons to whom the Software is
* furnished to do so, subject to the following conditions:
*
* The above copyright notice and this permission notice shall be included in
* all copies or substantial portions of the Software.
*
* THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
* IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
* FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
* AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
* LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
* OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
* THE SOFTWARE.
* =========================LICENSE_END==================================
*/
<template>
   <dlg ref="dialog" :title="`Logs of ${$route.params.service} in ${$route.params.app}`" :large="true" @close="clearLogs">
      <template v-slot:body>
        <div class="d-flex justify-content-end">
          <button type="button" class="btn btn-success" @click="processDownload"><font-awesome-icon
               icon="download" /> &nbsp;
            Download Logs</button>
        </div>
         <DynamicScroller ref="scroller" :items="logLines" :min-item-size="54" :item-size="20" class="ra-logs"
            :emit-update="true" :buffer="600">
            <template v-slot="{ item, index, active }">
               <DynamicScrollerItem :item="item" :active="active" :size-dependencies="[item.line,]"
                  :data-index="index" :data-active="active">
                  <div class="ra-log-line" :key="item.id">
                     {{ item.line }}
                  </div>
               </DynamicScrollerItem>
            </template>
         </DynamicScroller>
      </template>
   </dlg>
</template>

<style>
@import 'vue-virtual-scroller/dist/vue-virtual-scroller.css';

   .ra-logs {
      height: 80vh;
      overflow: auto;
      display: flex;
      flex-direction: column;
      background-color: black;
      color: white;
      font-family: var(--font-family-monospace);

      padding: 0.5rem;
   }

   .ra-log-line {
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
      height: 20px;
   }
</style>

<script>
import parseLinkHeader from 'parse-link-header';
import { DynamicScroller, DynamicScrollerItem } from 'vue-virtual-scroller';
import Dialog from './Dialog.vue';

let requestUri;

export default {
  data() {
    return {
      logLines: [],
      nextPageLink: null,
      since: '',
      limit: 2000,
      eventSource: null,
      scrollPosition: false,
      minItemSize: 24,
    };
  },
  components: {
    dlg: Dialog,
    DynamicScrollerItem: DynamicScrollerItem,
    DynamicScroller: DynamicScroller,
  },
  watch: {
    scrollPosition(scrollTopReached) {
      if (scrollTopReached === true) {
        this.limit = this.logLines.length + 2000;
        this.$nextTick(() => {
          this.eventSource.close();
          this.fetchLogs(this.currentPageLink, true);
        });
      }
    },
  },
  computed: {
    currentPageLink() {
      return `/api/apps/${this.$route.params.app}/logs/${this.$route.params.service}?limit=${this.limit}`;
    },
    downloadLink() {
      return `/api/apps/${this.$route.params.app}/logs/${this.$route.params.service}`;
    },
  },
  mounted() {
    this.fetchLogs(this.currentPageLink);
    this.$refs.scroller.$el.addEventListener('scroll', this.handleScroll);
  },
  beforeDestroy() {
    if (this.eventSource) {
      this.eventSource.close();
    }
    this.$refs.scroller.$el.removeEventListener('scroll');
  },
  methods: {
    fetchLogs(newRequestUri, reload = false) {
      if (newRequestUri == null) {
        return;
      }

      requestUri = newRequestUri;
      this.eventSource = new EventSource(requestUri);
      this.eventSource.onopen = () => {
        this.$refs.dialog.open();
      };

      this.eventSource.addEventListener('message', (e) => {
        const lines = e.data;
        const linesSplit = lines.split('\n');
        this.logLines = [];
        this.$nextTick(() => {
          this.logLines = this.logLines.concat(
            linesSplit
              .filter((line, index) => index < linesSplit.length - 1)
              .map((line, index) => ({ id: index, line }))
          );
          if (!reload) {
            this.scrollBottom();
          } else {
            this.$nextTick(() => {
              this.scrollToItem(2000);
            });
          }
          this.limit = this.logLines.length;
        });
      });

      this.eventSource.addEventListener('line', (e) => {
        const nextId = this.logLines.length > 0 ? this.logLines[this.logLines.length - 1].id + 1 : 1;
        this.logLines.push({ id: nextId, line: e.data });

        if (this.isCloseToBottom()) {
          this.$nextTick(() => {
            this.scrollBottom();
          });
        }
      });

      this.eventSource.onerror = function () {
        console.log('EventSource failed.');
      };
    },

    isCloseToBottom() {
      const el = this.$refs.scroller.$el;
      const distanceFromBottom =
        el.scrollHeight - (el.scrollTop + el.clientHeight);
      return distanceFromBottom < this.minItemSize;
    },

    clearLogs() {
      this.currentPageLink = null;
      this.nextPageLink = null;
      this.logLines = [];
      if (this.eventSource) {
        this.eventSource.close();
      }

      this.$router.push('/');
    },

    scrollBottom() {
      this.$refs.scroller.scrollToBottom();
    },
    scrollToItem(index) {
      this.$refs.scroller.scrollToItem(index);
    },

    processDownload() {
      fetch(this.downloadLink, {
        method: 'GET',
        headers: {
          Accept: 'text/plain',
        },
      })
        .then(parseLogsResponse)
        .then(({ logLines, rel }) => {
          const blob = new Blob([logLines], { type: 'text/plain' });
          const url = window.URL.createObjectURL(blob);

          const filename = `${this.$route.params.app}_${
            this.$route.params.service
          }_${new Date().toISOString()}.txt`;
          const link = document.createElement('a');
          link.href = url;
          link.download = filename;
          document.body.appendChild(link);
          link.click();

          document.body.removeChild(link);
          window.URL.revokeObjectURL(url);
        })
        .catch(() => {
          console.error('Unable to fetch logs for download');
        });
    },
    sendToBottom() {
      this.scrollBottom();
    },
    handleScroll() {
      const el = this.$refs.scroller.$el;
      this.limit = this.logLines.length;
      if (el.scrollTop === 0 && this.limit > 2000) {
        this.scrollPosition = true;
      } else {
        this.scrollPosition = false;
      }
    },
  },
};

function parseLogsResponse(response) {
  return new Promise((resolve, reject) => {
    if (!response.ok) {
      return reject(response);
    }

    const link = response.headers.get('Link');
    let rel = null;
    if (link != null) {
      const linkHeader = parseLinkHeader(link);
      if (linkHeader.next != null) {
        rel = linkHeader.next.url;
      }
    }
    return resolve(response.text().then((text) => ({ logLines: text, rel })));
  });
}
</script>
