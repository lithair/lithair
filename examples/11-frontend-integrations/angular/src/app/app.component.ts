import { Component, OnInit, inject } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { FormsModule } from '@angular/forms';
import { NgFor, NgIf, NgClass } from '@angular/common';
import { Note } from './note';

@Component({
  selector: 'app-root',
  standalone: true,
  imports: [FormsModule, NgFor, NgIf, NgClass],
  templateUrl: './app.component.html',
  styleUrl: './app.component.css'
})
export class AppComponent implements OnInit {
  private http = inject(HttpClient);

  notes: Note[] = [];
  title = '';
  content = '';

  ngOnInit(): void {
    this.fetchNotes();
  }

  fetchNotes(): void {
    this.http.get<Note[]>('/api/notes').subscribe({
      next: (notes) => {
        this.notes = notes;
      },
      error: (err) => {
        console.error('Failed to fetch notes:', err);
      }
    });
  }

  addNote(): void {
    const trimmedTitle = this.title.trim();
    if (!trimmedTitle) {
      return;
    }

    const body = {
      title: trimmedTitle,
      content: this.content.trim(),
      completed: false
    };

    this.http.post<Note>('/api/notes', body).subscribe({
      next: () => {
        this.title = '';
        this.content = '';
        this.fetchNotes();
      },
      error: (err) => {
        console.error('Failed to add note:', err);
      }
    });
  }

  toggleNote(note: Note): void {
    const body = {
      title: note.title,
      content: note.content,
      completed: !note.completed
    };

    this.http.put<Note>(`/api/notes/${note.id}`, body).subscribe({
      next: () => {
        this.fetchNotes();
      },
      error: (err) => {
        console.error('Failed to toggle note:', err);
      }
    });
  }

  deleteNote(id: string): void {
    this.http.delete(`/api/notes/${id}`).subscribe({
      next: () => {
        this.fetchNotes();
      },
      error: (err) => {
        console.error('Failed to delete note:', err);
      }
    });
  }
}
