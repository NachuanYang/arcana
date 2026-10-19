pub mod auth;
pub mod persistence;
pub mod reading;
pub mod tarot;

use std::sync::Arc;

pub fn run() {
    let tarot_deck = tarot::deck::TarotDeck::load_bundled()
        .expect("ARCANUM runtime tarot deck failed validation");
    tauri::Builder::default()
        .manage(Arc::new(auth::siwc::ChatGptAuthManager::default()))
        .manage(Arc::new(reading::tasks::ReadingTaskManager::default()))
        .manage(tarot::draw_engine::TarotDrawEngine::new(tarot_deck))
        .setup(|app| {
            use tauri::Manager;
            let directory = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            let repository =
                persistence::SessionRepository::open(directory.join("arcanum.sqlite3"))
                    .map_err(std::io::Error::other)?;
            app.manage(Arc::new(repository));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            auth::siwc::connect_chatgpt,
            auth::siwc::cancel_chatgpt_sign_in,
            auth::siwc::get_chatgpt_connection_status,
            auth::siwc::disconnect_chatgpt,
            auth::siwc::forget_chatgpt_registration,
            auth::siwc::open_chatgpt_usage,
            auth::inference::list_chatgpt_models,
            auth::inference::test_chatgpt_inference,
            persistence::commands::get_tarot_session,
            persistence::commands::list_tarot_sessions,
            persistence::commands::delete_tarot_session,
            reading::service::start_tarot_reading,
            reading::service::cancel_tarot_reading,
            reading::service::start_tarot_followup,
            reading::service::cancel_tarot_followup,
            tarot::draw_engine::create_tarot_draw,
            tarot::draw_engine::list_tarot_cards,
            tarot::draw_engine::reveal_tarot_draw,
            tarot::draw_engine::discard_tarot_draw
        ])
        .run(tauri::generate_context!())
        .expect("秘仪桌面应用启动失败");
}
