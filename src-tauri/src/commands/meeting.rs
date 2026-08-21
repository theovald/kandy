//! Tauri commands for the Meeting feature: record → transcribe → summarise.
//!
//! Recording drives `AudioRecordingManager` directly (binding id "meeting",
//! VAD disabled so the full meeting is captured, not just detected speech).
//! Transcription reuses the existing batch `TranscriptionManager::transcribe`.
//! Summaries go through the Kantega LLM proxy; if no key is configured the
//! transcript is still saved and returned (the mandatory no-key fallback).

use crate::audio_toolkit::{save_wav_file, VadPolicy};
use crate::kantega_llm::{self, MEETING_LLM_KEY_ID};
use crate::managers::audio::AudioRecordingManager;
use crate::managers::meeting::{Meeting, MeetingManager};
use crate::managers::transcription::TranscriptionManager;
use crate::settings::get_settings;
use chrono::Utc;
use std::sync::Arc;
use tauri::{AppHandle, State};

/// Whisper input rate — samples are already resampled to 16 kHz mono.
const SAMPLE_RATE: usize = 16000;

/// Binding id the meeting recorder uses so it is distinct from dictation.
const MEETING_BINDING: &str = "meeting";

#[tauri::command]
#[specta::specta]
pub async fn start_meeting_recording(
    recording_manager: State<'_, Arc<AudioRecordingManager>>,
) -> Result<(), String> {
    // VAD disabled: capture the whole meeting, including silences, so nothing is
    // trimmed before transcription.
    recording_manager
        .try_start_recording(MEETING_BINDING, VadPolicy::Disabled)
        .map(|_| ())
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_meeting_recording(
    recording_manager: State<'_, Arc<AudioRecordingManager>>,
) -> Result<(), String> {
    let generation = recording_manager.cancel_generation();
    recording_manager.stop_recording(MEETING_BINDING, generation);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn is_meeting_recording(recording_manager: State<'_, Arc<AudioRecordingManager>>) -> bool {
    recording_manager.is_recording()
}

/// Stop the meeting recording, save the audio, transcribe it, and persist the
/// meeting. Returns the saved meeting (without a summary yet).
#[tauri::command]
#[specta::specta]
pub async fn stop_meeting_recording(
    recording_manager: State<'_, Arc<AudioRecordingManager>>,
    transcription_manager: State<'_, Arc<TranscriptionManager>>,
    meeting_manager: State<'_, Arc<MeetingManager>>,
) -> Result<Meeting, String> {
    let generation = recording_manager.cancel_generation();
    let samples = recording_manager
        .stop_recording(MEETING_BINDING, generation)
        .ok_or_else(|| "No active meeting recording".to_string())?;

    if samples.is_empty() {
        return Err("Recording captured no audio".to_string());
    }

    let duration_secs = (samples.len() / SAMPLE_RATE) as i64;

    // Persist the WAV before the (slow) transcription so the audio survives even
    // if transcription fails.
    let file_name = format!("meeting_{}.wav", Utc::now().timestamp_millis());
    let audio_path = meeting_manager.audio_file_path(&file_name);
    save_wav_file(&audio_path, &samples).map_err(|e| format!("Failed to save audio: {}", e))?;

    transcription_manager.initiate_model_load();
    let tm = Arc::clone(&transcription_manager);
    let transcript = tauri::async_runtime::spawn_blocking(move || tm.transcribe(samples))
        .await
        .map_err(|e| format!("Transcription task panicked: {}", e))?
        .map_err(|e| e.to_string())?;

    meeting_manager
        .save_meeting(file_name, transcript, duration_secs)
        .map_err(|e| e.to_string())
}

/// Generate (or regenerate) a Norwegian summary for a saved meeting via the
/// Kantega LLM proxy, using the editable prompt + configured key/model.
#[tauri::command]
#[specta::specta]
pub async fn summarize_meeting(
    app: AppHandle,
    meeting_manager: State<'_, Arc<MeetingManager>>,
    id: i64,
) -> Result<Meeting, String> {
    let meeting = meeting_manager
        .get_by_id(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Meeting {} not found", id))?;

    if meeting.transcript.trim().is_empty() {
        return Err("Meeting has no transcript to summarise".to_string());
    }

    let settings = get_settings(&app);
    let api_key = settings
        .meeting_llm_api_keys
        .get(MEETING_LLM_KEY_ID)
        .cloned()
        .unwrap_or_default();
    let prompt = settings.meeting_summary_prompt.clone();
    let model = settings.meeting_summary_model.clone();

    let summary = kantega_llm::summarize(&api_key, &model, &prompt, &meeting.transcript).await?;

    meeting_manager
        .update_summary(id, summary, prompt, model)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_meetings(
    meeting_manager: State<'_, Arc<MeetingManager>>,
) -> Result<Vec<Meeting>, String> {
    meeting_manager.get_meetings().map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn delete_meeting(
    meeting_manager: State<'_, Arc<MeetingManager>>,
    id: i64,
) -> Result<(), String> {
    meeting_manager.delete(id).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn rename_meeting(
    meeting_manager: State<'_, Arc<MeetingManager>>,
    id: i64,
    title: String,
) -> Result<Meeting, String> {
    meeting_manager.rename(id, title).map_err(|e| e.to_string())
}
