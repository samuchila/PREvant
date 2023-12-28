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
         <DynamicScroller ref="scroller" :items="logLines" :min-item-size="24" class="ra-logs" :emit-update="true"
            @resize="scrollBottom" @scroll-end="handleEndScroll">
            <template v-slot="{ item, index, active }">
               <DynamicScrollerItem :item="item" :active="index === item.id" :size-dependencies="[item.line,]"
                  :data-index="item.id" :data-active="active">
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
   overflow-y: auto;
   background-color: black;
   color: white;
   font-family: var(--font-family-monospace);
   padding: 0.5rem;
}

.ra-log-line {
   margin-bottom: 5px;
   padding: 5px;
   white-space: nowrap;
   word-wrap: break-word;
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
         scrollPosition: 0,

      };
   },
   components: {
      'dlg': Dialog,
      'DynamicScrollerItem': DynamicScrollerItem,
      'DynamicScroller': DynamicScroller
   },
   watch: {
      currentPageLink(newCurrentPageLink) {
         this.logLines = [];
         this.fetchLogs(newCurrentPageLink);
      }
   },
   computed: {
      currentPageLink() {
         return `/api/apps/${this.$route.params.app}/logs/${this.$route.params.service}`;
      },
      filteredPageLink() {
         let queryString = '';
         queryString += this.since ? `since=${this.since}:00-00:00` : '';
         if (this.limit > 0) {
            queryString += queryString.length > 0 ? '&' : '';
            queryString += `limit=${this.limit}`;
         }

         return this.currentPageLink + (queryString ? `?${queryString}` : '');
      }
   },
   mounted() {
      this.fetchLogs(this.currentPageLink);

   },
   beforeDestroy() {
      if (this.eventSource) {
         this.eventSource.close();
      }
   },
   methods: {
      fetchLogs(newRequestUri) {
         if (newRequestUri == null || requestUri != null) {
            return;
         }

         requestUri = newRequestUri;
         this.eventSource = new EventSource(requestUri);
         this.eventSource.onopen = () => {
            this.$refs.dialog.open();
            console.log("Connection to server opened.");
         };


         this.eventSource.addEventListener("message", (e) => {
            const linesSplit = e.data.split('\n');
            this.logLines = this.logLines.concat(
               linesSplit
                  .filter((line, index) => index < linesSplit.length - 1)
                  .map((line, index) => ({ id: linesSplit.length - index, line }))
            );
         });
         this.eventSource.addEventListener("line", (e) => {
            this.logLines.push({ id: this.logLines.length + 1, line: e.data });

            if (this.isCloseToBottom()) {
               this.$nextTick(() => {
                  this.scrollBottom();
               });
            }
         });

         this.eventSource.onerror = function () {
            console.log("EventSource failed.");
         };


      },

      isCloseToBottom() {
         const el = this.$refs.scroller.$el;
         const distanceFromBottom = el.scrollHeight - (el.scrollTop + el.clientHeight);
         const minItemSize = 24;
         return distanceFromBottom < minItemSize;
      },

      clearLogs() {
         this.currentPageLink = null;
         this.nextPageLink = null;
         this.logLines = [];

         this.$router.push('/');
      },

      scrollBottom() {
         this.$refs.scroller.scrollToBottom();
      },

      sendRequest() {
         console.log('Sending request with:', this.since, this.limit);
         this.fetchLogs(this.filteredPageLink);
      },

      handleEndScroll() {
         console.log("Scroller at the bottom");
      },

      processDownload() {

         fetch(this.filteredPageLink)
            .then(parseLogsResponse)
            .then(({ logLines, rel }) => {
               const blob = new Blob([logLines], { type: 'text/plain' });
               const url = window.URL.createObjectURL(blob);

               const filename = `${this.$route.params.app}_${this.$route.params.service}_${new Date().toISOString()}.txt`;
               const link = document.createElement('a');
               link.href = url;
               link.download = filename;
               document.body.appendChild(link);
               link.click();

               document.body.removeChild(link);
               window.URL.revokeObjectURL(url);
            }
            )
            .catch(() => {
               console.error('Unable to fetch logs for download')
            });

      },

      updateLogs() {
         if (this.nextPageLink) {
            const nextPageLink = this.nextPageLink;
            this.nextPageLink = null;
            this.currentPageLink = nextPageLink;
         }
      },
      sendToBottom() {
         this.scrollBottom();
      }

   }
}

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
      return resolve(response.text().then(text => ({ logLines: text, rel })));
   });
}
</script>
