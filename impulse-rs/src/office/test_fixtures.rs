//! Documents the office tests read, built at test time.

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Writes a real OPC/OOXML document for reader tests, without a production writer.
/// `body` is XML assembled only from synthetic test literals below.
pub(crate) fn write_docx_body(path: &Path, body: &str) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
    );
    for (name, text) in [
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/document.xml", document.as_str()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

/// Writes a `.docx` with an `Intro` paragraph, one table row holding the
/// cells `A` and `B`, and `Outro`.
pub fn write_docx_with_table(dir: &Path) -> PathBuf {
    let path = dir.join("table.docx");
    write_docx_body(&path, "<w:p><w:r><w:t>Intro</w:t></w:r></w:p><w:tbl><w:tblPr/><w:tblGrid><w:gridCol/><w:gridCol/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>Outro</w:t></w:r></w:p>");
    path
}

/// Writes a `.docx` of `count` one-line paragraphs, `line 0` onward.
pub fn write_docx_lines(dir: &Path, count: usize) -> PathBuf {
    let path = dir.join("lines.docx");
    let body: String = (0..count)
        .map(|i| format!("<w:p><w:r><w:t>line {i}</w:t></w:r></w:p>"))
        .collect();
    write_docx_body(&path, &body);
    path
}

/// Writes a workbook whose `Data` sheet holds `start` at A1 and `end` at
/// zero-based row 3000, column 300, followed by an empty `Empty` sheet. A
/// dense grid of that sheet is about 900,000 cells, enough to tell dense
/// rendering from streaming without costing much memory if a regression
/// brings the dense grid back.
pub fn write_far_apart_workbook(dir: &Path) -> PathBuf {
    let path = dir.join("far.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let data = workbook.add_worksheet();
    data.set_name("Data").unwrap();
    data.write_string(0, 0, "start").unwrap();
    data.write_string(3000, 300, "end").unwrap();
    workbook.add_worksheet().set_name("Empty").unwrap();
    workbook.save(&path).unwrap();
    path
}

/// Copies the zip at `original` to `path`, replacing the text of each entry
/// for which `edit`, given the entry's name and text, returns new text.
pub fn rewrite_zip(original: &Path, path: &Path, edit: impl Fn(&str, &str) -> Option<String>) {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(original).unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry.name().unwrap().into_owned();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut data).unwrap();
        if let Some(text) = std::str::from_utf8(&data)
            .ok()
            .and_then(|text| edit(&name, text))
        {
            data = text.into_bytes();
        }
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap();
}

/// Writes `<stem>.original.xlsx`, a workbook with `x` in A1 of its one
/// sheet, and returns its path.
fn one_cell_workbook(dir: &Path, stem: &str) -> PathBuf {
    let path = dir.join(format!("{stem}.original.xlsx"));
    let mut workbook = rust_xlsxwriter::Workbook::new();
    workbook.add_worksheet().write_string(0, 0, "x").unwrap();
    workbook.save(&path).unwrap();
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
    seed.start_file("zeros.bin", zip::write::SimpleFileOptions::default())
        .unwrap();
    seed.write_all(&vec![0u8; MIB as usize]).unwrap();
    let seed = seed.finish().unwrap().into_inner();
    let mut seed = zip::ZipArchive::new(std::io::Cursor::new(seed)).unwrap();

    let path = dir.join("bomb.docx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    zip.start_file(
        "word/document.xml",
        zip::write::SimpleFileOptions::default(),
    )
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
