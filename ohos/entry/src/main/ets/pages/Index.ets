import { hilog } from '@kit.PerformanceAnalysisKit';

const DOMAIN: number = 0x0000;
const TAG: string = 'RareBox';

@Entry
@Component
struct Index {
  aboutToAppear(): void {
    hilog.info(DOMAIN, TAG, '%{public}s', 'RareBox Desktop page created');
  }

  build() {
    Stack({ alignContent: Alignment.TopStart }) {
      // 渲染面占位：后续由原生侧在 surface 上绘制。
      XComponent({
        id: 'rarebox',
        type: 'surface',
        libraryname: 'rarebox'
      })
        .width('100%')
        .height('100%')

      Text('RareBox Desktop')
        .fontSize(16)
        .fontColor('#FFFFFF')
        .margin({ top: 16, left: 16 })
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#000000')
  }
}
