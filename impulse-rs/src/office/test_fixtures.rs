//! Documents the office tests read, built at test time.

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Writes a `.docx` with an `Intro` paragraph, one table row holding the
/// cells `A` and `B`, and `Outro`.
pub fn write_docx_with_table(dir: &Path) -> PathBuf {
    use docx::{Docx, Paragraph, Run, Table, TableCell, TableRow};

    let cell = |text: &str| {
        TableCell::new().add_paragraph(Paragraph::new().add_run(Run::new().add_text(text)))
    };
    let path = dir.join("table.docx");
    Docx::new()
        .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Intro")))
        .add_table(Table::new(vec![TableRow::new(vec![cell("A"), cell("B")])]))
        .add_paragraph(Paragraph::new().add_run(Run::new().add_text("Outro")))
        .build()
        .pack(std::fs::File::create(&path).unwrap())
        .unwrap();
    path
}

/// Writes a `.docx` of `count` one-line paragraphs, `line 0` onward.
pub fn write_docx_lines(dir: &Path, count: usize) -> PathBuf {
    use docx::{Docx, Paragraph, Run};

    let path = dir.join("lines.docx");
    let mut docx = Docx::new();
    for i in 0..count {
        docx =
            docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(format!("line {i}"))));
    }
    docx.build()
        .pack(std::fs::File::create(&path).unwrap())
        .unwrap();
    path
}

/// Writes a workbook whose `Data` sheet holds `start` at A1 and `end` at
/// zero-based row 3000, column 300, followed by an empty `Empty` sheet. A
/// dense grid of that sheet is about 900,000 cells, enough to tell dense
/// rendering from streaming without costing much memory if a regression
/// brings the dense grid back.
pub fn write_far_apart_workbook(dir: &Path) -> PathBuf {
    let path = dir.join("far.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new(path.to_str().unwrap());
    let data = workbook.add_worksheet();
    data.set_name("Data").unwrap();
    data.write_string_only(0, 0, "start").unwrap();
    data.write_string_only(3000, 300, "end").unwrap();
    workbook.add_worksheet().set_name("Empty").unwrap();
    workbook.close().unwrap();
    path
}

/// Copies the zip at `original` to `path`, replacing the text of each entry
/// for which `edit`, given the entry's name and text, returns new text.
pub fn rewrite_zip(original: &Path, path: &Path, edit: impl Fn(&str, &str) -> Option<String>) {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(original).unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry.name().to_string();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut data).unwrap();
        if let Some(text) = std::str::from_utf8(&data)
            .ok()
            .and_then(|text| edit(&name, text))
        {
            data = text.into_bytes();
        }
        zip.start_file(name, zip::write::FileOptions::default())
            .unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap();
}

/// Writes `<stem>.original.xlsx`, a workbook with `x` in A1 of its one
/// sheet, and returns its path.
fn one_cell_workbook(dir: &Path, stem: &str) -> PathBuf {
    let path = dir.join(format!("{stem}.original.xlsx"));
    let mut workbook = rust_xlsxwriter::Workbook::new(path.to_str().unwrap());
    workbook
        .add_worksheet()
        .write_string_only(0, 0, "x")
        .unwrap();
    workbook.close().unwrap();
    path
}

/// Writes a one-cell workbook named `<name>.xlsx`, with its first sheet's
/// XML passed through `edit`, for malformed-input tests.
pub fn write_workbook_with_edited_sheet(
    dir: &Path,
    name: &str,
    edit: impl Fn(&str) -> String,
) -> PathBuf {
    let original = one_cell_workbook(dir, name);
    let path = dir.join(format!("{name}.xlsx"));
    rewrite_zip(&original, &path, |entry, xml| {
        (entry == "xl/worksheets/sheet1.xml").then(|| edit(xml))
    });
    path
}

/// Writes a workbook whose `xl/workbook.xml` lists `count` sheets, all of
/// them the same one-cell sheet part.
pub fn write_workbook_listing_sheets(dir: &Path, count: usize) -> PathBuf {
    let stem = format!("listing{count}");
    let original = one_cell_workbook(dir, &stem);
    let path = dir.join(format!("{stem}.xlsx"));
    rewrite_zip(&original, &path, |entry, xml| {
        (entry == "xl/workbook.xml").then(|| {
            let start = xml.find("<sheets>").unwrap() + "<sheets>".len();
            let end = xml.find("</sheets>").unwrap();
            let id_at = start + xml[start..end].find("r:id=\"").unwrap() + "r:id=\"".len();
            let id = &xml[id_at..id_at + xml[id_at..].find('"').unwrap()];
            let sheets: String = (1..=count)
                .map(|i| format!("<sheet name=\"S{i}\" sheetId=\"{i}\" r:id=\"{id}\"/>"))
                .collect();
            format!("{}{sheets}{}", &xml[..start], &xml[end..])
        })
    });
    path
}

/// Writes a `.docx` that is a few kilobytes on disk but whose entries
/// inflate to just past `max_inflated` bytes: one mebibyte of zeros is
/// compressed once and copied raw under new names. Only `word/document.xml`
/// is a real part, so a reader that inflates nothing else finds an
/// ordinary one-paragraph document.
pub fn write_docx_bomb(dir: &Path, max_inflated: u64) -> PathBuf {
    const MIB: u64 = 1 << 20;

    let mut seed = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    seed.start_file("zeros.bin", zip::write::FileOptions::default())
        .unwrap();
    seed.write_all(&vec![0u8; MIB as usize]).unwrap();
    let seed = seed.finish().unwrap().into_inner();
    let mut seed = zip::ZipArchive::new(std::io::Cursor::new(seed)).unwrap();

    let path = dir.join("bomb.docx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    zip.start_file("word/document.xml", zip::write::FileOptions::default())
        .unwrap();
    zip.write_all(
        br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>hi</w:t></w:r></w:p></w:body></w:document>"#,
    )
    .unwrap();
    for copy in 0..=max_inflated / MIB {
        zip.raw_copy_file_rename(
            seed.by_index(0).unwrap(),
            format!("word/media/zeros{copy}.bin"),
        )
        .unwrap();
    }
    zip.finish().unwrap();
    path
}
