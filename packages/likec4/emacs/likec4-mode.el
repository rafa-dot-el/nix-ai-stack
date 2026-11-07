;;; likec4-mode.el --- Major mode for LikeC4 architecture models  -*- lexical-binding: t; -*-

;; Shipped by the nix package packages/likec4 (share/emacs/site-lisp).
;; SPDX-License-Identifier: GPL-3.0-or-later

;;; Commentary:

;; Editing support for LikeC4 (.c4, .likec4) files: syntax table, font-lock,
;; indentation by braces, and registration of the LikeC4 language server
;; (`likec4 lsp --stdio`, the pinned binary from this package) with eglot and
;; lsp-mode.  The server provides validation, completion, go-to-definition,
;; references, hover, rename and formatting.
;;
;; Enable with (require 'likec4-mode); eglot starts with `M-x eglot' or
;; (add-hook 'likec4-mode-hook #'eglot-ensure).  The live diagram preview is
;; `likec4 start' in a terminal (http://localhost:5173).

;;; Code:

(defgroup likec4 nil
  "LikeC4 architecture-as-code."
  :group 'languages
  :prefix "likec4-")

(defcustom likec4-lsp-command '("@likec4@" "lsp" "--stdio")
  "Command that starts the LikeC4 language server."
  :type '(repeat string)
  :group 'likec4)

(defcustom likec4-indent-offset 2
  "Indentation step for LikeC4 blocks."
  :type 'integer
  :group 'likec4)

(defconst likec4--keywords
  '("specification" "model" "views" "deployment" "global" "extend" "import"
    "from" "element" "relationship" "deploymentNode" "tag" "view" "dynamic"
    "of" "include" "exclude" "style" "styleGroup" "predicateGroup" "with"
    "where" "is" "not" "and" "or" "instanceOf" "metadata" "description"
    "summary" "title" "technology" "link" "notation" "icon" "color" "shape"
    "size" "opacity" "border" "line" "head" "tail" "multiple" "autoLayout"
    "group" "parallel" "par" "alt" "when" "else" "loop" "opt" "break" "try"
    "catch" "finally" "variant" "navigateTo" "notes" "extends" "rank" "order"
    "includeAncestors" "kind" "source" "target")
  "LikeC4 keywords.")

(defconst likec4--font-lock
  `((,(regexp-opt likec4--keywords 'symbols) . font-lock-keyword-face)
    ("#[A-Za-z][-A-Za-z0-9_]*" . font-lock-type-face)
    ("^\\s-*\\([A-Za-z_][A-Za-z0-9_]*\\)\\s-*=\\s-*\\([a-z][-a-z0-9]*\\)"
     (1 font-lock-variable-name-face) (2 font-lock-type-face))
    ("-\\[\\([a-z][-a-z0-9]*\\)\\]->" 1 font-lock-builtin-face)
    ("\\_<\\(?:aws\\|azure\\|bootstrap\\|gcp\\|tech\\):[-A-Za-z0-9_]+" . font-lock-constant-face))
  "Font-lock rules for `likec4-mode'.")

(defvar likec4-mode-syntax-table
  (let ((st (make-syntax-table)))
    (modify-syntax-entry ?/ ". 124b" st)
    (modify-syntax-entry ?* ". 23" st)
    (modify-syntax-entry ?\n "> b" st)
    (modify-syntax-entry ?' "\"" st)
    (modify-syntax-entry ?\" "\"" st)
    (modify-syntax-entry ?_ "_" st)
    (modify-syntax-entry ?- "_" st)
    st)
  "Syntax table for `likec4-mode'.")

(defun likec4-indent-line ()
  "Indent the current line by brace depth."
  (interactive)
  (let ((depth (save-excursion
                 (beginning-of-line)
                 (car (syntax-ppss))))
        (closing (save-excursion
                   (back-to-indentation)
                   (looking-at-p "}"))))
    (indent-line-to (* likec4-indent-offset (max 0 (if closing (1- depth) depth))))))

;;;###autoload
(define-derived-mode likec4-mode prog-mode "LikeC4"
  "Major mode for LikeC4 architecture models."
  :syntax-table likec4-mode-syntax-table
  (setq-local comment-start "// ")
  (setq-local comment-end "")
  (setq-local font-lock-defaults '(likec4--font-lock))
  (setq-local indent-line-function #'likec4-indent-line))

;;;###autoload
(add-to-list 'auto-mode-alist '("\\.\\(?:c4\\|likec4\\)\\'" . likec4-mode))

(with-eval-after-load 'eglot
  (defvar eglot-server-programs)
  (add-to-list 'eglot-server-programs `(likec4-mode . ,likec4-lsp-command)))

(with-eval-after-load 'lsp-mode
  (defvar lsp-language-id-configuration)
  (declare-function lsp-register-client "lsp-mode")
  (declare-function make-lsp-client "lsp-mode")
  (declare-function lsp-stdio-connection "lsp-mode")
  (declare-function lsp-activate-on "lsp-mode")
  (add-to-list 'lsp-language-id-configuration '(likec4-mode . "likec4"))
  (lsp-register-client
   (make-lsp-client :new-connection (lsp-stdio-connection (lambda () likec4-lsp-command))
                    :activation-fn (lsp-activate-on "likec4")
                    :server-id 'likec4)))

(provide 'likec4-mode)
;;; likec4-mode.el ends here
