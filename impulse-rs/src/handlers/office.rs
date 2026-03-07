use anyhow::Result;

use crate::office;

use super::print_json;

pub fn handle_office(
    subcommand: String,
    file: Option<String>,
    goal: Option<String>,
    json: bool,
) -> Result<()> {
    match subcommand.as_str() {
        "info" | "status" => {
            println!("Office Document Support:");
            println!("  Formats: xlsx, xls, csv, docx");
            println!("  Status: Available (enable office-support feature for full functionality)");

            let formats = office::supported_formats();
            println!("\nSupported Formats:");
            println!(
                "  {:<10} {:<20} {:<10} {:<10}",
                "Extension", "Name", "Read", "Write"
            );
            println!("  {}", "-".repeat(50));
            for (ext, name, read, write) in formats {
                println!("  {:<10} {:<20} {:<10} {:<10}", ext, name, read, write);
            }
        }
        "parse" | "extract" => {
            let file = file.ok_or_else(|| anyhow::anyhow!("--file required"))?;
            let path = std::path::Path::new(&file);

            if !path.exists() {
                return Err(anyhow::anyhow!("File not found: {}", file));
            }

            let result = office::parse_document(path)
                .map_err(|e| anyhow::anyhow!("Failed to parse document: {}", e))?;

            if json {
                print_json(&result)?;
            } else {
                println!("Document: {}", result.metadata.source_path);
                println!("Type: {}", result.document_type);
                println!("Format: {}", result.metadata.format);
                println!("Size: {} bytes", result.metadata.size_bytes);
                println!("Chunks: {}", result.chunks.len());
                println!("\n--- Content Preview ---");
                let preview = result.content.chars().take(1000).collect::<String>();
                println!("{}", preview);
                if result.content.len() > 1000 {
                    println!("\n... (truncated, use --json for full content)");
                }
            }
        }
        "sheets" => {
            let file = file.ok_or_else(|| anyhow::anyhow!("--file required"))?;
            let path = std::path::Path::new(&file);

            if !path.exists() {
                return Err(anyhow::anyhow!("File not found: {}", file));
            }

            match office::excel::get_sheet_info(path) {
                Ok(sheets) => {
                    println!("Sheets in {}:", file);
                    for sheet in sheets {
                        println!(
                            "  - {} ({} rows x {} cols)",
                            sheet.name, sheet.row_count, sheet.column_count
                        );
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Failed to get sheet info: {}", e));
                }
            }
        }
        "chunk" => {
            let file = file.ok_or_else(|| anyhow::anyhow!("--file required"))?;
            let path = std::path::Path::new(&file);

            if !path.exists() {
                return Err(anyhow::anyhow!("File not found: {}", file));
            }

            let result = office::parse_document(path)
                .map_err(|e| anyhow::anyhow!("Failed to parse document: {}", e))?;

            let chunks = office::extraction::chunk_content(&result.content, 1000, 100);

            println!("Content split into {} chunks:", chunks.len());
            for (i, chunk) in chunks.iter().enumerate() {
                println!("\n--- Chunk {} ---", i);
                let preview = chunk.content.chars().take(200).collect::<String>();
                println!("{}", preview);
                if chunk.content.len() > 200 {
                    println!("...");
                }
            }
        }
        "extract-smart" | "smart" => {
            let file = file.ok_or_else(|| anyhow::anyhow!("--file required"))?;
            let goal = goal.unwrap_or_else(|| "extract all key information".to_string());
            let path = std::path::Path::new(&file);

            if !path.exists() {
                return Err(anyhow::anyhow!("File not found: {}", file));
            }

            let result = office::parse_document(path)
                .map_err(|e| anyhow::anyhow!("Failed to parse document: {}", e))?;

            let chunks = office::extraction::chunk_content(&result.content, 1000, 100);

            if json {
                let target = office::extraction::create_extraction_target(path, &goal)
                    .map_err(|e| anyhow::anyhow!("Failed to create extraction target: {}", e))?;
                print_json(&serde_json::json!({
                    "goal": goal,
                    "document_type": result.document_type,
                    "chunks": chunks.len(),
                    "content_length": result.content.len(),
                    "target": target,
                }))?;
            } else {
                println!("Smart extraction for goal: {}", goal);
                println!("Type: {}", result.document_type);
                println!("Chunks: {}", chunks.len());
                println!("Content length: {} characters", result.content.len());
            }
        }
        _ => {
            eprintln!(
                "Unknown office subcommand: {}. Use: info, parse, sheets, chunk, extract-smart",
                subcommand
            );
        }
    }
    Ok(())
}
