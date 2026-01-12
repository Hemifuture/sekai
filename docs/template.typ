// Sekai 技术文档模板
// 统一样式配置文件

#import "@preview/hydra:0.6.2": hydra
#import "@preview/cetz:0.4.2"

// 封面生成函数
#let cover(
  chinese_title: "",
  english_title: "",
  subtitle: "",
  version: "2.0",
  authors: (),
  date: datetime.today(),
) = {
  align(center)[
    #v(3cm)
    #text(size: 36pt, weight: "bold")[#chinese_title]
    #v(0.5cm)
    #text(size: 20pt)[#english_title]
    #if subtitle != "" {
      v(0.5cm)
      text(size: 14pt, fill: gray)[#subtitle]
    }
    #v(2cm)

    #line(length: 60%, stroke: 0.5pt + gray)

    #v(1cm)
    #text(size: 12pt)[
      *项目代号*: Sekai (日语: 世界) \
      *项目定位*: 自动生成拟真地形环境和政治板块的交互式地图编辑器 \
      *参考项目*: Azgaar's Fantasy Map Generator
    ]

    #if authors.len() > 0 {
      v(1.5cm)
      for author in authors [
        #text(size: 11pt)[#author] \
      ]
    }

    #v(3cm)
    #text(size: 10pt, fill: gray)[
      文档版本: #version \
      最后更新: #date.display("[year]-[month]-[day]")
    ]
  ]
  pagebreak()
}

// 主文档模板函数
#let doc(
  title: "",
  version: "2.0",
  authors: (),
  body,
) = {
  // 设置文档元数据
  set document(
    title: title,
    author: if authors.len() > 0 { authors.at(0) } else { "" },
    date: datetime.today(),
  )

  // === 基础文本样式 ===
  // 使用字体回退：中文优先使用 Noto Serif SC，英文回退到 Times New Roman
  set text(
    font: ("Times New Roman", "Noto Serif SC"),
    size: 14pt,
    lang: "zh",
    region: "CN"
  )

  // 粗体使用无衬线字体
  show strong: set text(
    font: "Noto Sans SC",
    weight: "regular",
    size: 14pt,
    lang: "zh",
    region: "CN"
  )

  // 链接样式
  show link: set text(fill: rgb("#6b38ff"))

  // === 段落样式 ===
  set par(
    justify: true,
    leading: 0.8em,
  )

  // === 页面样式 ===
  set page(
    paper: "a4",
    margin: (x: 2.5cm, y: 2cm),
    numbering: "1",
    header: context {
      if counter(page).get().first() > 1 [
        #text(size: 9pt, fill: gray)[#title]
        #h(1fr)
        #text(size: 9pt, fill: gray)[v#version]
        #v(-0.3em)
        #line(length: 100%, stroke: 0.5pt + gray)
      ]
    },
    footer: context {
      let num = counter(page).get().first()
      if num > 1 {
        if calc.odd(num) {
          h(1fr)
          counter(page).display("- 1 -", both: false)
        } else {
          counter(page).display("- 1 -", both: false)
          h(1fr)
        }
      }
    },
  )

  // === 标题样式 ===
  set heading(numbering: "1.1")

  // 一级标题 - 大标题，前面弱分页
  show heading.where(level: 1): it => {
    pagebreak(weak: true)
    v(1em)
    text(size: 20pt, weight: "bold", font: "Noto Sans SC", it)
    v(0.5em)
  }

  // 二级标题 - 中等大小
  show heading.where(level: 2): it => {
    v(0.8em)
    text(size: 16pt, weight: "bold", font: "Noto Sans SC", it)
    v(0.4em)
  }

  // 三级标题 - 较小
  show heading.where(level: 3): it => {
    v(0.6em)
    text(size: 14pt, weight: "medium", font: "Noto Sans SC", it)
    v(0.3em)
  }

  // === 代码块样式 ===
  // 块级代码
  show raw.where(block: true): it => {
    set text(size: 9pt, font: "Fira Code")
    block(
      fill: luma(245),
      inset: 10pt,
      radius: 4pt,
      width: 100%,
      stroke: 0.5pt + luma(220),
      it
    )
  }

  // 行内代码
  show raw.where(block: false): it => {
    box(
      fill: luma(240),
      inset: (x: 3pt, y: 0pt),
      outset: (y: 3pt),
      radius: 2pt,
      it
    )
  }

  // === 表格样式 ===
  show table: set text(size: 11pt)
  show table.cell: it => {
    if it.y == 0 {
      set text(weight: "bold", font: "Noto Sans SC")
      it
    } else {
      it
    }
  }

  // === 图表样式 ===
  show figure: set block(breakable: false)
  show figure.caption: set text(size: 11pt, style: "italic")

  // 生成封面
  cover(
    chinese_title: "Sekai",
    english_title: "拟真地图生成器",
    subtitle: "技术文档",
    version: version,
    authors: authors,
  )

  // 生成目录
  outline(
    title: [目录],
    indent: 2em,
    depth: 3,
  )

  pagebreak()

  // 正文内容
  body
}
