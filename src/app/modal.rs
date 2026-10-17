//! O que captura o teclado além do modo normal. Só um modal existe por vez.

use super::*;

pub(crate) enum Modal {
    None,
    /// Texto do prompt `/` em edição.
    Search(String),
    TableZoom(Zoom),
    ImageZoom(imgview::ImgZoom),
    /// Seletor de arquivos (`mdv <pasta>`); ocupa a tela enquanto existe.
    Picker(picker::Picker),
    /// URL externa esperando confirmação (`y`).
    ConfirmOpen(String),
}

impl App {
    pub(crate) fn zoom(&self) -> Option<&Zoom> {
        match &self.modal {
            Modal::TableZoom(z) => Some(z),
            _ => None,
        }
    }

    pub(crate) fn zoom_mut(&mut self) -> Option<&mut Zoom> {
        match &mut self.modal {
            Modal::TableZoom(z) => Some(z),
            _ => None,
        }
    }

    pub(crate) fn img_zoom(&self) -> Option<&imgview::ImgZoom> {
        match &self.modal {
            Modal::ImageZoom(z) => Some(z),
            _ => None,
        }
    }

    pub(crate) fn img_zoom_mut(&mut self) -> Option<&mut imgview::ImgZoom> {
        match &mut self.modal {
            Modal::ImageZoom(z) => Some(z),
            _ => None,
        }
    }

    pub(crate) fn picker(&self) -> Option<&picker::Picker> {
        match &self.modal {
            Modal::Picker(p) => Some(p),
            _ => None,
        }
    }

    pub(crate) fn picker_mut(&mut self) -> Option<&mut picker::Picker> {
        match &mut self.modal {
            Modal::Picker(p) => Some(p),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn pending_open(&self) -> Option<&str> {
        match &self.modal {
            Modal::ConfirmOpen(url) => Some(url),
            _ => None,
        }
    }

    pub(crate) fn search_input(&self) -> Option<&str> {
        match &self.modal {
            Modal::Search(buf) => Some(buf),
            _ => None,
        }
    }
}
