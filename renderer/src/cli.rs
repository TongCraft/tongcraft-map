use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use tc_map_core::config::Project;
use tc_map_core::inspect;
use tc_map_core::render::{render, Backend};
use tc_map_core::verify;

#[derive(Debug, Parser)]
#[command(name = "tc-map", version, about = "TongCraft Map 离线 CPU 制图工具")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 检查快照目录、版本和区块编码，不出图
    Inspect {
        #[arg(long)]
        snapshot: PathBuf,
        #[arg(long, default_value = "map.toml")]
        config: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// 渲染一个完整候选版本到 output/staging/<id>
    Render {
        #[arg(long)]
        snapshot: PathBuf,
        #[arg(long, default_value = "map.toml")]
        config: PathBuf,
        #[arg(long, value_enum, default_value = "cpu")]
        backend: BackendArg,
        #[arg(long)]
        release: String,
        #[arg(long)]
        rebuild: bool,
        #[arg(long)]
        json: bool,
    },
    /// 校验候选版本的清单、覆盖和瓦片
    Verify {
        #[arg(long = "release-dir")]
        release_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BackendArg {
    Cpu,
    Gpu,
    Auto,
}

impl From<BackendArg> for Backend {
    fn from(value: BackendArg) -> Self {
        match value {
            BackendArg::Cpu => Backend::Cpu,
            BackendArg::Gpu => Backend::Gpu,
            BackendArg::Auto => Backend::Auto,
        }
    }
}

pub fn run() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Inspect {
            snapshot,
            config,
            json,
        } => {
            let project = Project::load(&snapshot, &config)?;
            let report = inspect::inspect(&project)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("快照：{}", report.snapshot);
                println!(
                    "样式 {} · CPU 线程 {} · GPU {}",
                    report.style_version, report.threads, report.hardware.gpu
                );
                for dimension in &report.dimensions {
                    println!(
                        "\n{}（{}）\n  区域目录：{} · region 文件 {} · 区块 {}",
                        dimension.id,
                        dimension.layout,
                        dimension.region_dir,
                        dimension.region_files,
                        dimension.chunks
                    );
                    if let Some(bounds) = dimension.bounds {
                        println!("  范围：{bounds:?}");
                    }
                    println!("  状态：{:?}", dimension.status);
                    println!("  版本：{:?}", dimension.data_versions);
                    println!("  压缩：{:?}", dimension.compression);
                    if dimension.external_chunks > 0 {
                        println!("  外置区块：{}", dimension.external_chunks);
                    }
                }
            }
        }
        Command::Render {
            snapshot,
            config,
            backend,
            release,
            rebuild,
            json,
        } => {
            let project = Project::load(&snapshot, &config)?;
            let summary = render(&project, &release, backend.into(), rebuild)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&summary)?);
            } else {
                println!("候选版本：{}", summary.staging_dir);
                println!(
                    "{} 个维度 · {} 个瓦片 · {:.1} MiB · {:.1}s",
                    summary.dimension_count,
                    summary.tiles,
                    summary.tile_bytes as f64 / 1024.0 / 1024.0,
                    summary.duration_ms as f64 / 1000.0
                );
                for warning in &summary.warnings {
                    eprintln!("警告：{warning}");
                }
            }
        }
        Command::Verify { release_dir, json } => {
            let report = verify::verify(&release_dir)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "版本 {} 通过：{} 个维度 · {} 个瓦片（抽样解码 {}）· {} 个标记",
                    report.release,
                    report.dimensions.len(),
                    report.tiles,
                    report.sampled_tiles,
                    report.markers
                );
            }
        }
    }
    Ok(())
}
